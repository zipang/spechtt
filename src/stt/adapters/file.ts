/**
 * File-session adapter: transcribe a local audio or video file.
 *
 * Unlike a live session, nothing is pushed over the bridge: the worker owns a
 * path, decodes it with ffmpeg ([`decodeFileToPcm16`]) and feeds the PCM to the
 * same [`Segmenter`] the live adapters use, so silence alignment, hangover,
 * segment merging and junction dedupe are shared rather than reimplemented.
 *
 * Two deliberate differences from the live cadence:
 *
 * - Segmentation is **file-tuned** ([`FILE_SEGMENTATION`]): larger segments mean
 *   fewer requests and more context for the model, and batch transcription is
 *   faster than realtime so latency does not matter.
 * - Segments are transcribed **strictly sequentially**, and the decoder is not
 *   pulled again until the previous segment has been transcribed, so memory
 *   stays bounded by one segment no matter how long the recording is.
 *
 * The session never emits `partial` or `delta`: there is no live hypothesis.
 */

import type { AudioSegment, SegmenterOptions } from "../segmentation";
import { Segmenter } from "../segmentation";
import type { TranscriptionModel } from "ai";
import { FileDecodeError } from "../file-input";
import type { OutboundEvent, SttErrorCode } from "../protocol";
import { mergeTranscript } from "../transcript-merge";
import { pcm16ToWav } from "./mistral";
import type { EmitFn, SttSession, SttSessionConfig } from "./types";
import type { TranscribeFn } from "./mistral";

/**
 * Segmentation cadence used for files.
 *
 * `targetMs`/`maxSegmentMs` are raised well above the live 3 s / 7 s cadence;
 * every other knob (silence alignment, hangover, min-speech merge, overlap)
 * keeps its [`Segmenter`] default.
 */
export const FILE_SEGMENTATION = {
	targetMs: 30_000,
	maxSegmentMs: 60_000,
} as const;

/** Injectable collaborators; every one is faked in tests. */
export interface FileSessionDeps {
	/** Streams 16 kHz mono PCM16 bytes decoded from `path`. */
	decode(
		path: string,
		options: { signal: AbortSignal },
	): AsyncIterable<Uint8Array>;
	/** Best-effort container duration in seconds. */
	probeDuration(path: string, signal: AbortSignal): Promise<number | null>;
	transcribe: TranscribeFn;
	resolveModel(config: SttSessionConfig): TranscriptionModel;
	/** Overrides [`FILE_SEGMENTATION`] (tests use a much shorter cadence). */
	segmentation?: Partial<Omit<SegmenterOptions, "sampleRate">>;
}

function errorMessage(error: unknown): string {
	if (error instanceof Error && error.message.length > 0) {
		return error.message;
	}
	if (typeof error === "string" && error.length > 0) {
		return error;
	}
	return "File transcription failed";
}

/** A decode failure keeps its wire code; anything else is `stream_failed`. */
function errorCode(error: unknown): SttErrorCode {
	return error instanceof FileDecodeError ? error.code : "stream_failed";
}

class FileSession implements SttSession {
	private readonly segmenter: Segmenter;
	private readonly path: string;
	private readonly decodeController = new AbortController();
	private readonly transcribeController = new AbortController();
	private readonly runPromise: Promise<void>;

	private readonly config: SttSessionConfig;
	private accumulated = "";
	private decodedSamples = 0;
	private lastProcessedSeconds = 0;
	private totalSeconds: number | null = null;
	private aborted = false;
	private done = false;
	private errorEmitted = false;

	constructor(
		readonly sessionId: string,
		config: SttSessionConfig & { path: string },
		private readonly emit: EmitFn,
		private readonly deps: FileSessionDeps,
	) {
		this.path = config.path;
		this.config = config;
		this.segmenter = new Segmenter({
			sampleRate: config.sampleRate,
			...FILE_SEGMENTATION,
			...deps.segmentation,
		});
		this.runPromise = this.run();
	}

	/**
	 * No-ops: a file session has no live audio input. They exist because
	 * [`SttSession`] is shared with the streaming adapters.
	 */
	pushAudio(): void {}

	pause(): void {}

	resume(): void {}

	async stop(): Promise<void> {
		if (this.done) {
			return;
		}
		// Stop pulling the decoder, but keep transcribing what was already cut.
		this.decodeController.abort();
		await this.runPromise;
	}

	async abort(): Promise<void> {
		if (this.aborted) {
			return;
		}
		this.aborted = true;
		this.transcribeController.abort();
		this.decodeController.abort();
		await this.runPromise;
	}

	/** Probes, emits `started`, decodes, transcribes and emits `stopped`. */
	private async run(): Promise<void> {
		try {
			let model: TranscriptionModel;
			try {
				model = this.deps.resolveModel(this.config);
			} catch (error) {
				this.emitError(error);
				return;
			}

			this.totalSeconds = await this.deps
				.probeDuration(this.path, this.decodeController.signal)
				.catch(() => null);
			if (this.aborted) {
				return;
			}

			this.emit({ type: "started", sessionId: this.sessionId });

			const chunks = this.deps.decode(this.path, {
				signal: this.decodeController.signal,
			});
			try {
				for await (const chunk of chunks) {
					if (this.aborted || this.decodeController.signal.aborted) {
						break;
					}
					if (chunk.length === 0) {
						continue;
					}

					this.decodedSamples += Math.floor(chunk.length / 2);
					for (const segment of this.segmenter.push(chunk)) {
						await this.transcribeSegment(segment, model);
						if (this.aborted) {
							return;
						}
						this.emitProgress();
					}
				}
			} catch (error) {
				if (this.aborted) {
					return;
				}
				// A container that fails mid-file still yields a usable
				// transcript: report the failure, then finalize what decoded.
				this.emitError(error);
			}

			if (this.aborted) {
				return;
			}

			// End of decode, a decode failure, or a `stop()` mid-file: flush
			// the segment still buffered so no audio is silently dropped.
			for (const segment of this.segmenter.flush("stop")) {
				await this.transcribeSegment(segment, model);
				if (this.aborted) {
					return;
				}
				this.emitProgress();
			}

			this.emit({
				type: "stopped",
				sessionId: this.sessionId,
				fullText: this.accumulated,
			});
		} catch (error) {
			if (this.aborted) {
				return;
			}
			this.emitError(error);
		} finally {
			this.done = true;
		}
	}

	/** WAV-frames one segment, transcribes it and commits the new text only. */
	private async transcribeSegment(
		segment: AudioSegment,
		model: TranscriptionModel,
	): Promise<void> {
		if (segment.pcm16.length === 0) {
			return;
		}

		console.error(
			`[stt-worker] file segment index=${segment.index} reason=${segment.reason} durationMs=${segment.durationMs} speechMs=${segment.speechMs} rmsDb=${segment.rmsDb.toFixed(1)} overlap=${segment.startsWithOverlap}`,
		);

		const wav = pcm16ToWav(segment.pcm16, this.config.sampleRate);
		const result = await this.deps.transcribe({
			model,
			audio: wav,
			abortSignal: this.transcribeController.signal,
		});
		if (this.aborted) {
			return;
		}

		const text = typeof result.text === "string" ? result.text : "";
		if (text.trim().length === 0) {
			return;
		}

		const merged = mergeTranscript(this.accumulated, text, {
			allowSingleWordOverlap: segment.startsWithOverlap,
		});
		const appended = merged.slice(this.accumulated.length).trim();
		this.accumulated = merged;
		if (appended.length === 0) {
			return;
		}

		this.emit({
			type: "committed",
			sessionId: this.sessionId,
			text: appended,
		});
	}

	/**
	 * Reports the decoded position. Monotonic by construction: a reading that
	 * does not advance is skipped, so the UI never jumps backwards.
	 */
	private emitProgress(): void {
		const processedSeconds = this.decodedSamples / this.config.sampleRate;
		if (processedSeconds <= this.lastProcessedSeconds) {
			return;
		}
		this.lastProcessedSeconds = processedSeconds;
		this.emit({
			type: "progress",
			sessionId: this.sessionId,
			processedSeconds,
			totalSeconds: this.totalSeconds,
		});
	}

	private emitError(error: unknown): void {
		if (this.errorEmitted) {
			return;
		}
		this.errorEmitted = true;
		this.emit({
			type: "error",
			sessionId: this.sessionId,
			code: errorCode(error),
			message: errorMessage(error),
		});
	}
}

/**
 * Creates a session that transcribes `config.path`.
 *
 * Work starts immediately (probe → `started` → decode loop); `stop()` and
 * `abort()` both await it. `abort()` emits nothing.
 */
export function createFileSession(
	config: SttSessionConfig & { path: string },
	emit: EmitFn,
	deps: FileSessionDeps,
): SttSession {
	return new FileSession(config.sessionId, config, emit, deps);
}
