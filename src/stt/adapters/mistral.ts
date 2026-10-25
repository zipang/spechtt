import { createMistral } from "@ai-sdk/mistral";
import { type TranscriptionModel, transcribe as aiTranscribe } from "ai";
import type { OutboundEvent } from "../protocol";
import type { TranscriptionMode } from "../registry";
import { appendSegment, defaultModelResolver } from "./vercel-ai-sdk";
import type {
	EmitFn,
	SttAdapter,
	SttSession,
	SttSessionConfig,
} from "./types";

export type TranscribeFn = typeof aiTranscribe;

export type MistralModelResolver = (
	config: SttSessionConfig,
) => TranscriptionModel;

export interface MistralAdapterOptions {
	/** Injectable for tests; defaults to the AI SDK `transcribe`. */
	transcribe?: TranscribeFn;
	/** Injectable for tests; defaults to a `@ai-sdk/mistral` model. */
	resolveModel?: MistralModelResolver;
	/** Target segment duration before an automatic flush (default 2.5 s). */
	segmentSeconds?: number;
}

function writeAscii(target: Uint8Array, offset: number, value: string): void {
	for (let index = 0; index < value.length; index += 1) {
		target[offset + index] = value.charCodeAt(index);
	}
}

/**
 * Minimal 44-byte canonical WAV header for 16-bit PCM.
 */
export function createWavHeader(
	dataLength: number,
	sampleRate: number,
	channels = 1,
	bitsPerSample = 16,
): Uint8Array {
	const header = new Uint8Array(44);
	const view = new DataView(header.buffer);
	const blockAlign = channels * (bitsPerSample / 8);

	writeAscii(header, 0, "RIFF");
	view.setUint32(4, 36 + dataLength, true);
	writeAscii(header, 8, "WAVE");
	writeAscii(header, 12, "fmt ");
	view.setUint32(16, 16, true);
	view.setUint16(20, 1, true); // PCM
	view.setUint16(22, channels, true);
	view.setUint32(24, sampleRate, true);
	view.setUint32(28, sampleRate * blockAlign, true);
	view.setUint16(32, blockAlign, true);
	view.setUint16(34, bitsPerSample, true);
	writeAscii(header, 36, "data");
	view.setUint32(40, dataLength, true);

	return header;
}

/** Decodes base64 into raw PCM16 bytes. */
export function pcm16Base64ToPcm16(pcm16Base64: string): Uint8Array {
	const binary = atob(pcm16Base64);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return bytes;
}

/** Wraps raw PCM16 little-endian mono bytes in a WAV container. */
export function pcm16ToWav(pcm16: Uint8Array, sampleRate: number): Uint8Array {
	const wav = new Uint8Array(44 + pcm16.length);
	wav.set(createWavHeader(pcm16.length, sampleRate), 0);
	wav.set(pcm16, 44);
	return wav;
}

/** Decodes a base64 PCM16 chunk and frames it as a WAV segment. */
export function pcm16Base64ToWav(
	pcm16Base64: string,
	sampleRate: number,
): Uint8Array {
	return pcm16ToWav(pcm16Base64ToPcm16(pcm16Base64), sampleRate);
}

function errorMessage(error: unknown): string {
	if (error instanceof Error && error.message.length > 0) {
		return error.message;
	}
	if (typeof error === "string" && error.length > 0) {
		return error;
	}
	return "Batch transcription failed";
}

function stripProviderPrefix(provider: string, model: string): string {
	const prefix = `${provider}/`;
	return model.startsWith(prefix) ? model.slice(prefix.length) : model;
}

export function defaultMistralModel(
	config: SttSessionConfig,
): TranscriptionModel {
	return createMistral({ apiKey: config.apiKey }).transcription(
		stripProviderPrefix(config.provider, config.model) as never,
	);
}

/**
 * Segmented models exist across providers (`mistral/voxtral-*`,
 * `openai/whisper-1`, ...): use the Mistral SDK for direct Mistral sessions and
 * the shared provider resolver otherwise.
 */
function defaultSegmentedModel(
	config: SttSessionConfig,
): TranscriptionModel {
	return config.provider === "mistral"
		? defaultMistralModel(config)
		: defaultModelResolver(config);
}

class FailedSegmentedSession implements SttSession {
	private stopped = false;

	constructor(
		readonly sessionId: string,
		private readonly emit: EmitFn,
	) {}

	pushAudio(): void {}

	pause(): void {}

	resume(): void {}

	async stop(): Promise<void> {
		if (this.stopped) {
			return;
		}
		this.stopped = true;
		this.emit({
			type: "stopped",
			sessionId: this.sessionId,
			fullText: "",
		});
	}

	async abort(): Promise<void> {}
}

class MistralSession implements SttSession {
	private chunks: Uint8Array[] = [];
	private bufferLength = 0;
	private accumulated = "";
	private paused = false;
	private aborted = false;
	private stopped = false;
	private pending: Promise<void> = Promise.resolve();
	private errorEmitted = false;
	private readonly abortController = new AbortController();
	private readonly segmentBytes: number;

	constructor(
		readonly sessionId: string,
		private readonly emit: EmitFn,
		private readonly transcribe: TranscribeFn,
		private readonly model: TranscriptionModel,
		private readonly sampleRate: number,
		segmentSeconds: number,
	) {
		this.segmentBytes = Math.max(
			1,
			Math.round(sampleRate * 2 * segmentSeconds),
		);
	}

	pushAudio(pcm16Base64: string): void {
		if (this.paused || this.aborted || this.stopped) {
			return;
		}

		const pcm = pcm16Base64ToPcm16(pcm16Base64);
		if (pcm.length === 0) {
			return;
		}
		this.chunks.push(pcm);
		this.bufferLength += pcm.length;

		if (this.bufferLength >= this.segmentBytes) {
			void this.queueFlush();
		}
	}

	pause(): void {
		this.paused = true;
		void this.queueFlush();
	}

	resume(): void {
		this.paused = false;
	}

	async stop(): Promise<void> {
		if (this.stopped || this.aborted) {
			return;
		}
		this.stopped = true;

		await this.queueFlush();
		await this.pending;

		if (this.aborted) {
			return;
		}
		this.emit({
			type: "stopped",
			sessionId: this.sessionId,
			fullText: this.accumulated,
		});
	}

	async abort(reason?: unknown): Promise<void> {
		if (this.aborted) {
			return;
		}
		this.aborted = true;
		this.clearBuffer();
		this.abortController.abort(reason);
	}

	private clearBuffer(): void {
		this.chunks = [];
		this.bufferLength = 0;
	}

	private takeBuffer(): Uint8Array {
		if (this.bufferLength === 0) {
			return new Uint8Array();
		}

		const pcm = new Uint8Array(this.bufferLength);
		let offset = 0;
		for (const chunk of this.chunks) {
			pcm.set(chunk, offset);
			offset += chunk.length;
		}
		this.clearBuffer();
		return pcm;
	}

	private queueFlush(): Promise<void> {
		if (this.bufferLength === 0) {
			return this.pending;
		}

		const pcm = this.takeBuffer();
		const wav = pcm16ToWav(pcm, this.sampleRate);

		this.pending = this.pending.then(async () => {
			if (this.aborted) {
				return;
			}

			try {
				const result = await this.transcribe({
					model: this.model,
					audio: wav,
					abortSignal: this.abortController.signal,
				});
				if (this.aborted) {
					return;
				}

				const text = typeof result.text === "string" ? result.text : "";
				if (text.trim().length === 0) {
					return;
				}

				this.accumulated = appendSegment(this.accumulated, text);
				this.emit({
					type: "committed",
					sessionId: this.sessionId,
					text,
				});
			} catch (error) {
				if (this.aborted) {
					return;
				}
				this.emitError(errorMessage(error));
			}
		});

		return this.pending;
	}

	private emitError(message: string): void {
		if (this.errorEmitted) {
			return;
		}
		this.errorEmitted = true;
		const event: OutboundEvent = {
			type: "error",
			sessionId: this.sessionId,
			code: "stream_failed",
			message,
		};
		this.emit(event);
	}
}

/**
 * Batch/segmented adapter for Mistral Voxtral (and other `transcribe`-only
 * models). PCM16 chunks accumulate until a segment threshold, a `pause`, or a
 * `stop`; each segment is WAV-framed before the AI SDK `transcribe` call.
 */
export class MistralAdapter implements SttAdapter {
	readonly mode: TranscriptionMode = "segmented";

	private readonly transcribe: TranscribeFn;
	private readonly resolveModel: MistralModelResolver;
	private readonly segmentSeconds: number;

	constructor(options: MistralAdapterOptions = {}) {
		this.transcribe = options.transcribe ?? aiTranscribe;
		this.resolveModel = options.resolveModel ?? defaultSegmentedModel;
		this.segmentSeconds = options.segmentSeconds ?? 2.5;
	}

	start(config: SttSessionConfig, emit: EmitFn): SttSession {
		let model: TranscriptionModel;
		try {
			model = this.resolveModel(config);
		} catch (error) {
			emit({
				type: "error",
				sessionId: config.sessionId,
				code: "stream_failed",
				message: errorMessage(error),
			});
			return new FailedSegmentedSession(config.sessionId, emit);
		}

		emit({ type: "started", sessionId: config.sessionId });
		return new MistralSession(
			config.sessionId,
			emit,
			this.transcribe,
			model,
			config.sampleRate,
			this.segmentSeconds,
		);
	}
}
