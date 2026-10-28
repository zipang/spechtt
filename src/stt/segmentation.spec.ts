import { describe, expect, test } from "bun:test";
import {
	type AudioSegment,
	DEFAULT_MAX_SEGMENT_MS,
	DEFAULT_MIN_SPEECH_MS,
	DEFAULT_PRE_ROLL_MS,
	DEFAULT_TAIL_PAD_MS,
	Segmenter,
	type SegmenterOptions,
} from "./segmentation";

const RATE = 16_000;
const FRAME_BYTES = (RATE * 20) / 1000 * 2;

function silence(ms: number): Uint8Array {
	return new Uint8Array(Math.round((RATE * ms) / 1000) * 2);
}

function tone(ms: number, amplitude = 0.5, frequency = 220): Uint8Array {
	const samples = Math.round((RATE * ms) / 1000);
	const bytes = new Uint8Array(samples * 2);
	const view = new DataView(bytes.buffer);
	for (let index = 0; index < samples; index += 1) {
		const value =
			Math.sin((2 * Math.PI * frequency * index) / RATE) *
			amplitude *
			Number(i16Max());
		view.setInt16(index * 2, Math.round(value), true);
	}
	return bytes;
}

function i16Max(): number {
	return 32_767;
}

/** Deterministic broadband-ish signal, used as an ambient-noise stand-in. */
function noise(ms: number, amplitude = 0.02): Uint8Array {
	const samples = Math.round((RATE * ms) / 1000);
	const bytes = new Uint8Array(samples * 2);
	const view = new DataView(bytes.buffer);
	for (let index = 0; index < samples; index += 1) {
		const value =
			(Math.sin((2 * Math.PI * 137 * index) / RATE) +
				Math.sin((2 * Math.PI * 911 * index) / RATE)) *
			amplitude *
			Number(i16Max());
		view.setInt16(index * 2, Math.round(value), true);
	}
	return bytes;
}

function concat(...parts: Uint8Array[]): Uint8Array {
	const length = parts.reduce((total, part) => total + part.length, 0);
	const combined = new Uint8Array(length);
	let offset = 0;
	for (const part of parts) {
		combined.set(part, offset);
		offset += part.length;
	}
	return combined;
}

function make(overrides: Partial<SegmenterOptions> = {}): Segmenter {
	return new Segmenter({ sampleRate: RATE, ...overrides });
}

function pushAll(
	target: Segmenter,
	bytes: Uint8Array,
	chunkSize = FRAME_BYTES,
): AudioSegment[] {
	const segments: AudioSegment[] = [];
	for (let offset = 0; offset < bytes.length; offset += chunkSize) {
		segments.push(
			...target.push(
				bytes.subarray(offset, Math.min(offset + chunkSize, bytes.length)),
			),
		);
	}
	return segments;
}

/** Asserts that a byte range is digital silence. */
function expectSilence(bytes: Uint8Array, startMs: number, endMs: number): void {
	const start = Math.round((RATE * startMs) / 1000) * 2;
	const end = Math.round((RATE * endMs) / 1000) * 2;
	for (let offset = start; offset < end; offset += 1) {
		if (bytes[offset] !== 0) {
			throw new Error(`expected silence at byte ${offset} (${startMs}-${endMs} ms)`);
		}
	}
}

describe("Segmenter", () => {
	test("never emits a segment for silence", () => {
		const segmenter = make();
		expect(pushAll(segmenter, silence(3000))).toEqual([]);
		expect(segmenter.flush("stop")).toEqual([]);
	});

	test("detects loud speech at the very first frame", () => {
		const segmenter = make();
		const segments = pushAll(segmenter, concat(tone(1500), silence(700)));

		expect(segments).toHaveLength(1);
		expect(segments[0]?.speechMs).toBe(1500);
		expect(segments[0]?.reason).toBe("silence");
		expect(segments[0]?.startsWithOverlap).toBe(false);
	});

	test("pads segments with pre-roll and tail silence", () => {
		const segmenter = make();
		const segments = pushAll(
			segmenter,
			concat(silence(400), tone(1500), silence(800)),
		);

		expect(segments).toHaveLength(1);
		const segment = segments[0];
		expect(segment?.speechMs).toBe(1500);
		// 300 ms pre-roll + 1500 ms speech + 200 ms tail pad.
		expect(segment?.durationMs).toBe(2000);
		expectSilence(segment?.pcm16 ?? new Uint8Array(), 0, DEFAULT_PRE_ROLL_MS);
		expectSilence(
			segment?.pcm16 ?? new Uint8Array(),
			1800,
			1800 + DEFAULT_TAIL_PAD_MS,
		);
		expect(segment?.rmsDb).toBeLessThan(-6);
	});

	test("splits sentences separated by a pause into separate segments", () => {
		const segmenter = make();
		const audio = concat(
			tone(1500),
			silence(600),
			tone(1500),
			silence(600),
			tone(1500),
			silence(600),
		);
		const segments = pushAll(segmenter, audio);

		expect(segments).toHaveLength(3);
		expect(segments.map((segment) => segment.speechMs)).toEqual([
			1500, 1500, 1500,
		]);
		expect(segments.map((segment) => segment.reason)).toEqual([
			"silence",
			"silence",
			"silence",
		]);
		expect(segments.map((segment) => segment.index)).toEqual([0, 1, 2]);
		expect(segmenter.flush("stop")).toEqual([]);
	});

	test("merges a short blip into the next segment instead of transcribing it", () => {
		const segmenter = make();
		const segments = pushAll(
			segmenter,
			concat(tone(200), silence(600), tone(1500), silence(600)),
		);

		expect(segments).toHaveLength(1);
		expect(segments[0]?.speechMs).toBe(1700);
	});

	test("keeps short voiced audio when the user stops right after it", () => {
		const segmenter = make();
		const segments = pushAll(segmenter, concat(tone(200), silence(1000)));
		const tail = segmenter.flush("stop");

		expect(segments).toEqual([]);
		expect(tail).toHaveLength(1);
		expect(tail[0]?.reason).toBe("stop");
		expect(tail[0]?.speechMs).toBe(200);
	});

	test("pause pads and closes; resume starts a fresh segment", () => {
		const segmenter = make();
		segmenter.push(tone(1000));
		const first = segmenter.flush("pause");

		segmenter.push(tone(1000));
		const second = segmenter.flush("stop");

		expect(first).toHaveLength(1);
		expect(first[0]?.reason).toBe("pause");
		expect(first[0]?.speechMs).toBe(1000);
		expect(first[0]?.durationMs).toBe(1000 + DEFAULT_TAIL_PAD_MS);
		expect(second).toHaveLength(1);
		expect(second[0]?.reason).toBe("stop");
		expect(second[0]?.index).toBe(1);
	});

	test("forces a cut near the cap and repeats a short overlap", () => {
		const segmenter = make();
		const collected = [
			...pushAll(segmenter, tone(12_000)),
			...segmenter.flush("stop"),
		];

		expect(collected).toHaveLength(2);
		const [first, second] = collected;
		expect(first?.reason).toBe("max");
		expect(first?.startsWithOverlap).toBe(false);
		expect(first?.durationMs).toBeLessThanOrEqual(DEFAULT_MAX_SEGMENT_MS);
		expect(second?.startsWithOverlap).toBe(true);
		expect(second?.reason).toBe("stop");

		// 13 frames of 20 ms at 16 kHz, rounded from the 250 ms default.
		const overlapBytes = 13 * ((RATE * 20) / 1000) * 2;
		expect(second?.pcm16.slice(0, overlapBytes)).toEqual(
			first?.pcm16.slice(-overlapBytes),
		);
	});

	test("cuts at the quietest frame near the cap", () => {
		const segmenter = make();
		const audio = concat(tone(6600), tone(20, 0.001), tone(880));
		const segments = pushAll(segmenter, audio);
		const collected = [...segments, ...segmenter.flush("stop")];

		expect(collected).toHaveLength(2);
		expect(collected[0]?.reason).toBe("max");
		expect(collected[0]?.durationMs).toBe(6600);
		expect(collected[0]?.speechMs).toBe(6600);
		expect(collected[1]?.startsWithOverlap).toBe(true);
		expect(collected[1]?.speechMs).toBe(880);
	});

	test("produces identical segments for any input chunking", () => {
		const audio = concat(
			tone(1500),
			silence(600),
			tone(1500),
			silence(600),
			tone(1500),
			silence(600),
		);

		const reference = pushAll(make(), audio, FRAME_BYTES);
		for (const chunk of [1, 7, 641, 1024, 5000]) {
			const other = pushAll(make(), audio, chunk);
			expect(other.map((segment) => [segment.durationMs, segment.speechMs])).toEqual(
				reference.map((segment) => [segment.durationMs, segment.speechMs]),
			);
			other.forEach((segment, index) => {
				expect(segment.pcm16).toEqual(reference[index]?.pcm16);
			});
		}
	});

	test("does not mistake steady ambient noise for speech", () => {
		const segmenter = make();
		const segments = pushAll(
			segmenter,
			concat(noise(2000), tone(1500, 0.3), silence(700)),
		);

		expect(segments).toHaveLength(1);
		expect(segments[0]?.speechMs).toBe(1500);
	});

	test("reset restores indices and discards buffered audio", () => {
		const segmenter = make();
		const first = pushAll(segmenter, concat(tone(1500), silence(700)));
		expect(first[0]?.index).toBe(0);

		segmenter.reset();
		const second = pushAll(segmenter, concat(tone(1500), silence(700)));
		expect(second[0]?.index).toBe(0);
		expect(second[0]?.speechMs).toBe(first[0]?.speechMs);
	});

	test("uses the minimum speech limit from the options", () => {
		const segmenter = make({ minSpeechMs: DEFAULT_MIN_SPEECH_MS * 2 });
		const segments = pushAll(
			segmenter,
			concat(tone(1500), silence(600), tone(1500), silence(600)),
		);

		expect(segments).toHaveLength(1);
		expect(segments[0]?.speechMs).toBe(3000);
	});
});
