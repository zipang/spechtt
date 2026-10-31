/**
 * Silence-aligned audio segmentation for batch transcription.
 *
 * The segmenter turns a continuous 16 kHz mono PCM16 stream into segments that
 * start and end inside silence, so batch models (`transcribe`) receive whole
 * phrases instead of arbitrary byte-count windows. It is pure and
 * deterministic: every decision depends on sample counts and frame energies,
 * never on the wall clock, randomness, or I/O.
 */

/** Analysis window. */
export const DEFAULT_FRAME_MS = 20;
/** Audio kept before speech onset, so the first phoneme is never clipped. */
export const DEFAULT_PRE_ROLL_MS = 300;
/** Silence appended after the last speech frame. */
export const DEFAULT_TAIL_PAD_MS = 200;
/** Voiced audio below this is merged forward instead of transcribed alone. */
export const DEFAULT_MIN_SPEECH_MS = 1200;
/** Nominal segment length, reported for diagnostics only. */
export const DEFAULT_TARGET_MS = 3000;
/** Hard cap that forces a cut even without a pause. */
export const DEFAULT_MAX_SEGMENT_MS = 7000;
/** Silence run that closes a segment. */
export const DEFAULT_HANGOVER_MS = 500;
/** Audio repeated at the start of the segment after a forced cut. */
export const DEFAULT_OVERLAP_MS = 250;

/** Window searched for the quietest frame on a forced cut. */
const FORCED_CUT_SEARCH_MS = 600;
/** The very end of the buffer is never used as a cut point. */
const FORCED_CUT_EXCLUDE_MS = 100;
/** Absolute floor (≈ -60 dBFS) that keeps digital silence from triggering. */
const MIN_ABSOLUTE_RMS = 0.001;
/** Initial noise-floor estimate (≈ -60 dBFS). */
const INITIAL_NOISE_FLOOR = 0.001;
const MIN_NOISE_FLOOR = 1e-5;
const MAX_NOISE_FLOOR = 0.05;
/** Frames used to calibrate the floor before the first speech decision. */
const CALIBRATION_FRAMES = 5;
/** A frame this loud (≈ -26 dBFS) is speech even during calibration. */
const STRONG_SPEECH_RMS = 0.05;
const MIN_DB = -120;

export type SegmentReason = "silence" | "max" | "pause" | "stop";

/** One WAV-ready chunk of PCM16 little-endian mono audio. */
export interface AudioSegment {
	readonly pcm16: Uint8Array;
	/** Monotonic index within this segmenter (0-based). */
	readonly index: number;
	readonly reason: SegmentReason;
	/** True when this segment repeats audio from the previous one. */
	readonly startsWithOverlap: boolean;
	/** Voiced duration, excluding pre-roll and tail padding. */
	readonly speechMs: number;
	/** Total duration, including padding and overlap. */
	readonly durationMs: number;
	/** Mean energy of voiced frames, in dBFS (negative). */
	readonly rmsDb: number;
}

export interface SegmenterOptions {
	readonly sampleRate: number;
	readonly frameMs?: number;
	readonly preRollMs?: number;
	readonly tailPadMs?: number;
	readonly minSpeechMs?: number;
	readonly targetMs?: number;
	readonly maxSegmentMs?: number;
	readonly hangoverMs?: number;
	readonly overlapMs?: number;
	readonly noiseFloorAlpha?: number;
	readonly speechTriggerRatio?: number;
	readonly silenceRatio?: number;
}

interface Frame {
	readonly samples: Int16Array;
	readonly rms: number;
	/** Classified as speech when the frame was consumed. */
	speech: boolean;
}

function clamp(value: number, min: number, max: number): number {
	return Math.min(max, Math.max(min, value));
}

function toDb(rms: number): number {
	return rms <= 0 ? MIN_DB : Math.max(MIN_DB, 20 * Math.log10(rms));
}

function concatBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
	if (left.length === 0) {
		return right.slice();
	}
	if (right.length === 0) {
		return left;
	}
	const combined = new Uint8Array(left.length + right.length);
	combined.set(left, 0);
	combined.set(right, left.length);
	return combined;
}

function frameCount(ms: number, frameMs: number, minimum = 1): number {
	return Math.max(minimum, Math.round(ms / frameMs));
}

/** Accumulates PCM16 and emits silence-aligned segments. */
export class Segmenter {
	private readonly sampleRate: number;
	private readonly frameMs: number;
	private readonly frameSamples: number;
	private readonly frameBytes: number;
	private readonly preRollFrames: number;
	private readonly tailPadFrames: number;
	private readonly minSpeechFrames: number;
	private readonly maxFrames: number;
	private readonly hangoverFrames: number;
	private readonly overlapFrames: number;
	private readonly searchFrames: number;
	private readonly excludeFrames: number;
	private readonly alpha: number;
	private readonly onsetRatio: number;
	private readonly silenceRatio: number;

	private pending = new Uint8Array(0);
	private preRoll: Frame[] = [];
	/** Voiced audio retained from a segment shorter than the minimum. */
	private carry: Frame[] = [];
	private frames: Frame[] = [];
	private inSpeech = false;
	private silenceFrames = 0;
	private nextIndex = 0;
	private noiseFloor = INITIAL_NOISE_FLOOR;
	private overlapPending = false;
	private calibrationFrames = CALIBRATION_FRAMES;
	private calibrationMin = Number.POSITIVE_INFINITY;

	constructor(options: SegmenterOptions) {
		this.sampleRate = options.sampleRate;
		this.frameMs = options.frameMs ?? DEFAULT_FRAME_MS;
		this.frameSamples = Math.max(
			1,
			Math.round((this.sampleRate * this.frameMs) / 1000),
		);
		this.frameBytes = this.frameSamples * 2;
		this.preRollFrames = frameCount(
			options.preRollMs ?? DEFAULT_PRE_ROLL_MS,
			this.frameMs,
		);
		this.tailPadFrames = frameCount(
			options.tailPadMs ?? DEFAULT_TAIL_PAD_MS,
			this.frameMs,
		);
		this.minSpeechFrames = frameCount(
			options.minSpeechMs ?? DEFAULT_MIN_SPEECH_MS,
			this.frameMs,
		);
		this.searchFrames = frameCount(FORCED_CUT_SEARCH_MS, this.frameMs);
		this.excludeFrames = frameCount(FORCED_CUT_EXCLUDE_MS, this.frameMs);
		this.maxFrames = Math.max(
			frameCount(options.maxSegmentMs ?? DEFAULT_MAX_SEGMENT_MS, this.frameMs),
			this.searchFrames + this.excludeFrames + 1,
		);
		this.hangoverFrames = frameCount(
			options.hangoverMs ?? DEFAULT_HANGOVER_MS,
			this.frameMs,
		);
		this.overlapFrames = frameCount(
			options.overlapMs ?? DEFAULT_OVERLAP_MS,
			this.frameMs,
		);
		this.alpha = options.noiseFloorAlpha ?? 0.05;
		this.onsetRatio = options.speechTriggerRatio ?? 2.5;
		this.silenceRatio = options.silenceRatio ?? 1.5;
	}

	/** Appends PCM16 LE bytes and returns every segment completed by this push. */
	push(pcm16: Uint8Array): AudioSegment[] {
		if (pcm16.length === 0) {
			return [];
		}

		const combined = concatBytes(this.pending, pcm16);
		const emitted: AudioSegment[] = [];
		let offset = 0;

		while (offset + this.frameBytes <= combined.length) {
			const frame = this.toFrame(
				combined.subarray(offset, offset + this.frameBytes),
			);
			offset += this.frameBytes;
			this.consume(frame, emitted);
		}

		this.pending =
			offset === 0 ? combined.slice() : combined.slice(offset);
		return emitted;
	}

	/**
	 * Closes the open segment, padding it with silence.
	 *
	 * `pause` and `stop` are the only valid reasons. Voiced audio is always
	 * emitted, even when shorter than `minSpeechMs`; silence is dropped.
	 */
	flush(reason: "pause" | "stop"): AudioSegment[] {
		const emitted: AudioSegment[] = [];

		if (this.pending.length > 0) {
			const frame = this.toFrame(this.pending);
			this.pending = new Uint8Array(0);
			this.consume(frame, emitted);
		}

		const source = this.inSpeech ? this.frames : this.carry;
		if (source.length > 0 && this.speechFrames(source) > 0) {
			const padded = [...source, ...this.padding()];
			this.emit(padded, reason, emitted);
		}

		this.inSpeech = false;
		this.frames = [];
		this.carry = [];
		this.preRoll = [];
		this.silenceFrames = 0;
		this.overlapPending = false;
		return emitted;
	}

	/** Drops buffered audio and restores the initial state. */
	reset(): void {
		this.pending = new Uint8Array(0);
		this.preRoll = [];
		this.carry = [];
		this.frames = [];
		this.inSpeech = false;
		this.silenceFrames = 0;
		this.nextIndex = 0;
		this.noiseFloor = INITIAL_NOISE_FLOOR;
		this.overlapPending = false;
		this.calibrationFrames = CALIBRATION_FRAMES;
		this.calibrationMin = Number.POSITIVE_INFINITY;
	}

	private toFrame(bytes: Uint8Array): Frame {
		const count = bytes.length >> 1;
		const samples = new Int16Array(count);
		const view = new DataView(bytes.buffer, bytes.byteOffset, count * 2);
		let sumSquares = 0;

		for (let index = 0; index < count; index += 1) {
			const sample = view.getInt16(index * 2, true);
			samples[index] = sample;
			const normalized = sample / 32768;
			sumSquares += normalized * normalized;
		}

		const rms = count === 0 ? 0 : Math.sqrt(sumSquares / count);
		return { samples, rms, speech: false };
	}

	private padding(): Frame[] {
		const pad: Frame[] = [];
		for (let index = 0; index < this.tailPadFrames; index += 1) {
			pad.push({
				samples: new Int16Array(this.frameSamples),
				rms: 0,
				speech: false,
			});
		}
		return pad;
	}

	private consume(frame: Frame, emitted: AudioSegment[]): void {
		const onsetThreshold = Math.max(
			this.noiseFloor * this.onsetRatio,
			MIN_ABSOLUTE_RMS,
		);
		const silenceThreshold = this.noiseFloor * this.silenceRatio;

		if (!this.inSpeech) {
			if (this.calibrationFrames > 0) {
				this.calibrationMin = Math.min(this.calibrationMin, frame.rms);
				this.calibrationFrames -= 1;
				if (this.calibrationFrames === 0) {
					this.noiseFloor = clamp(
						Math.max(this.noiseFloor, this.calibrationMin),
						MIN_NOISE_FLOOR,
						MAX_NOISE_FLOOR,
					);
				}
				if (frame.rms <= STRONG_SPEECH_RMS) {
					frame.speech = false;
					this.preRoll.push(frame);
					if (this.preRoll.length > this.preRollFrames) {
						this.preRoll.shift();
					}
					return;
				}
			}

			if (frame.rms > onsetThreshold) {
				frame.speech = true;
				this.calibrationFrames = 0;
				this.calibrationMin = Number.POSITIVE_INFINITY;
				this.frames = [...this.carry, ...this.preRoll, frame];
				this.carry = [];
				this.preRoll = [];
				this.inSpeech = true;
				this.silenceFrames = 0;
				this.checkForcedCut(emitted);
				return;
			}

			frame.speech = false;
			this.noiseFloor = clamp(
				this.noiseFloor * (1 - this.alpha) + frame.rms * this.alpha,
				MIN_NOISE_FLOOR,
				MAX_NOISE_FLOOR,
			);
			this.preRoll.push(frame);
			if (this.preRoll.length > this.preRollFrames) {
				this.preRoll.shift();
			}
			return;
		}

		frame.speech = frame.rms >= silenceThreshold;
		this.frames.push(frame);

		if (frame.speech) {
			this.silenceFrames = 0;
			this.checkForcedCut(emitted);
			return;
		}

		this.silenceFrames += 1;
		if (this.silenceFrames >= this.hangoverFrames) {
			this.closeOnSilence(emitted);
		}
	}

	private closeOnSilence(emitted: AudioSegment[]): void {
		const runStart = this.frames.length - this.silenceFrames;
		const cut = Math.min(this.frames.length, runStart + this.tailPadFrames);
		const audio = this.frames.slice(0, cut);
		const speechMs = this.speechMs(audio);

		if (speechMs === 0) {
			this.frames = [];
			this.inSpeech = false;
			this.silenceFrames = 0;
			return;
		}

		if (speechMs < this.minSpeechFrames * this.frameMs) {
			// Too short to stand alone: keep it and prepend it to the next one.
			this.carry = audio.slice(
				Math.max(0, audio.length - (this.preRollFrames + this.minSpeechFrames)),
			);
			this.frames = [];
			this.inSpeech = false;
			this.silenceFrames = 0;
			return;
		}

		this.emit(audio, "silence", emitted);
		this.frames = [];
		this.inSpeech = false;
		this.silenceFrames = 0;
	}

	private checkForcedCut(emitted: AudioSegment[]): void {
		if (this.frames.length < this.maxFrames) {
			return;
		}

		const start = Math.max(0, this.frames.length - this.searchFrames);
		const end = Math.max(
			start + 1,
			Math.min(this.frames.length - 1, this.frames.length - this.excludeFrames),
		);

		let quietest = start;
		for (let index = start; index < end; index += 1) {
			if (this.frames[index].rms < this.frames[quietest].rms) {
				quietest = index;
			}
		}

		const cut = clamp(quietest, 1, this.frames.length - 1);
		const head = this.frames.slice(0, cut);
		const overlapStart = Math.max(0, cut - this.overlapFrames);
		const tail = this.frames.slice(overlapStart);

		if (this.speechMs(head) > 0) {
			this.emit(head, "max", emitted);
			this.overlapPending = true;
		}

		// The repeated lead-in is context, not new speech: it must not inflate
		// `speechMs` or trigger a minimum-length merge forward.
		for (let index = 0; index < cut - overlapStart; index += 1) {
			tail[index].speech = false;
		}

		this.frames = tail;
		this.silenceFrames = 0;
	}

	private speechMs(frames: readonly Frame[]): number {
		return this.speechFrames(frames) * this.frameMs;
	}

	private speechFrames(frames: readonly Frame[]): number {
		let count = 0;
		for (const frame of frames) {
			if (frame.speech) {
				count += 1;
			}
		}
		return count;
	}

	private emit(
		frames: readonly Frame[],
		reason: SegmentReason,
		emitted: AudioSegment[],
	): void {
		const speechFrames = this.speechFrames(frames);
		if (speechFrames === 0) {
			return;
		}

		let totalSamples = 0;
		for (const frame of frames) {
			totalSamples += frame.samples.length;
		}
		const pcm16 = new Uint8Array(totalSamples * 2);
		const view = new DataView(pcm16.buffer);
		let offset = 0;
		let sumSquares = 0;

		for (const frame of frames) {
			if (frame.speech) {
				sumSquares += frame.rms * frame.rms;
			}
			for (const sample of frame.samples) {
				view.setInt16(offset, sample, true);
				offset += 2;
			}
		}

		emitted.push({
			pcm16,
			index: this.nextIndex,
			reason,
			startsWithOverlap: this.overlapPending,
			speechMs: speechFrames * this.frameMs,
			durationMs: frames.length * this.frameMs,
			rmsDb: toDb(Math.sqrt(sumSquares / speechFrames)),
		});

		this.nextIndex += 1;
		this.overlapPending = false;
	}
}
