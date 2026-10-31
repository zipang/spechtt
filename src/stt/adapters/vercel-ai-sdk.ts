import { createElevenLabs } from "@ai-sdk/elevenlabs";
import { createMistral } from "@ai-sdk/mistral";
import { createOpenAI } from "@ai-sdk/openai";
import { createXai } from "@ai-sdk/xai";
import {
	type StreamTranscriptionResult,
	type TranscriptionModel,
	createGateway,
	experimental_streamTranscribe,
} from "ai";
import type { OutboundEvent } from "../protocol";
import type { TranscriptionMode } from "../registry";
import { AudioStreamBridge } from "../stream-controller";
import type {
	EmitFn,
	SttAdapter,
	SttSession,
	SttSessionConfig,
} from "./types";

export type StreamTranscribeFn = (
	options: Parameters<typeof experimental_streamTranscribe>[0],
) => StreamTranscriptionResult;

export type ModelResolver = (config: SttSessionConfig) => TranscriptionModel;

export interface VercelAiSdkAdapterOptions {
	/** Injectable for tests; defaults to `experimental_streamTranscribe`. */
	streamTranscribe?: StreamTranscribeFn;
	/** Injectable for tests; defaults to gateway/provider model instances. */
	resolveModel?: ModelResolver;
}

function errorMessage(error: unknown): string {
	if (error instanceof Error && error.message.length > 0) {
		return error.message;
	}
	if (typeof error === "string" && error.length > 0) {
		return error;
	}
	return "Transcription stream failed";
}

/**
 * Provider phrasings that mean "the stream carried nothing to transcribe".
 * A take that produced no text and failed this way is a normal, silent
 * outcome: the provider did not fail, it simply had no speech to work on.
 */
const NOTHING_TO_TRANSCRIBE_PATTERNS = [
	"no audio",
	"no speech",
	"no sound",
	"no voice",
	"no transcript",
	"no transcription",
	"nothing to transcribe",
	"nothing was recorded",
	"nothing recorded",
	"empty audio",
	"audio is empty",
	"audio was empty",
	"silent audio",
	"no valid audio",
	"must contain audio",
];

/** Whether a provider message reports an empty/voiceless input. */
export function isEmptyInputFailure(message: string): boolean {
	const lowered = message.toLowerCase();
	return NOTHING_TO_TRANSCRIBE_PATTERNS.some((pattern) =>
		lowered.includes(pattern),
	);
}

/** Joins finalized segments with single spaces. */
export function appendSegment(accumulated: string, text: string): string {
	const trimmed = text.trim();
	if (trimmed.length === 0) {
		return accumulated;
	}
	return accumulated.length === 0 ? trimmed : `${accumulated} ${trimmed}`;
}

function stripProviderPrefix(provider: string, model: string): string {
	const prefix = `${provider}/`;
	return model.startsWith(prefix) ? model.slice(prefix.length) : model;
}

function gatewayModelId(config: SttSessionConfig): string {
	if (config.model.includes("/")) {
		return config.model;
	}
	if (config.provider === "vercel" || config.provider === "gateway") {
		return config.model;
	}
	return `${config.provider}/${config.model}`;
}

/**
 * Resolves the model instance for a session. Direct providers use their own
 * SDK so the `start.apiKey` override is honored; Vercel/gateway sessions go
 * through `createGateway`.
 */
export function defaultModelResolver(
	config: SttSessionConfig,
): TranscriptionModel {
	const { provider, model, apiKey } = config;

	switch (provider) {
		case "mistral":
			return createMistral({ apiKey }).transcription(
				stripProviderPrefix(provider, model) as never,
			);

		case "openai":
			return createOpenAI({ apiKey }).transcription(
				stripProviderPrefix(provider, model) as never,
			);

		case "xai":
			return createXai({ apiKey }).transcription(
				stripProviderPrefix(provider, model) as never,
			);

		case "elevenlabs":
			return createElevenLabs({ apiKey }).transcription(
				stripProviderPrefix(provider, model) as never,
			);

		default:
			return createGateway({ apiKey }).transcription(
				gatewayModelId(config) as never,
			);
	}
}

class FailedStreamingSession implements SttSession {
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

class VercelAiSdkSession implements SttSession {
	private readonly done: Promise<void>;
	private resolveDone: () => void = () => {};
	private accumulated = "";
	private aborted = false;
	private stoppedEmitted = false;
	private errorEmitted = false;
	/** A silent take must never surface a provider flush failure. */
	private receivedAudio = false;
	private producedText = false;

	constructor(
		readonly sessionId: string,
		private readonly emit: EmitFn,
		private readonly bridge: AudioStreamBridge,
		result: StreamTranscriptionResult,
	) {
		this.done = new Promise<void>((resolve) => {
			this.resolveDone = resolve;
		});
		void this.consume(result);
	}

	pushAudio(pcm16Base64: string): void {
		if (pcm16Base64.length === 0) {
			return;
		}
		if (this.bridge.pushAudio(pcm16Base64)) {
			this.receivedAudio = true;
		}
	}

	pause(): void {
		this.bridge.pause();
	}

	resume(): void {
		this.bridge.resume();
	}

	async stop(): Promise<void> {
		this.bridge.close();
		await this.done;
	}

	async abort(reason?: unknown): Promise<void> {
		if (this.aborted) {
			return;
		}
		this.aborted = true;
		this.bridge.abort(reason);
	}

	private emitStopped(fullText: string): void {
		if (this.stoppedEmitted) {
			return;
		}
		this.stoppedEmitted = true;
		this.emit({
			type: "stopped",
			sessionId: this.sessionId,
			fullText,
		});
	}

	private emitError(message: string): void {
		if (this.errorEmitted) {
			return;
		}
		// A provider failure only matters once there was something to
		// transcribe: no audio at all, or a silent take whose only text was
		// the provider reporting that it had nothing to work on.
		if (!this.producedText) {
			if (!this.receivedAudio || isEmptyInputFailure(message)) {
				return;
			}
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

	private async consume(result: StreamTranscriptionResult): Promise<void> {
		try {
			for await (const part of result.fullStream) {
				if (this.aborted) {
					return;
				}

				switch (part.type) {
					case "transcript-partial":
						this.producedText ||= part.text.trim().length > 0;
						this.emit({
							type: "partial",
							sessionId: this.sessionId,
							text: part.text,
						});
						break;

					case "transcript-delta":
						this.producedText ||= part.delta.trim().length > 0;
						this.emit({
							type: "delta",
							sessionId: this.sessionId,
							delta: part.delta,
						});
						break;

					case "transcript-final":
						this.producedText ||= part.text.trim().length > 0;
						this.accumulated = appendSegment(
							this.accumulated,
							part.text,
						);
						this.emit({
							type: "committed",
							sessionId: this.sessionId,
							text: part.text,
						});
						break;

					case "error":
						this.emitError(errorMessage(part.error));
						break;
				}
			}

			if (this.aborted) {
				return;
			}

			let finalText = this.accumulated;
			try {
				const providerText = await result.text;
				if (typeof providerText === "string" && providerText.length > 0) {
					this.producedText = true;
					finalText = providerText;
				}
			} catch {
				// The provider stream failed: keep the text accumulated so far.
			}
			if (this.aborted) {
				return;
			}
			this.emitStopped(finalText);
		} catch (error) {
			if (this.aborted) {
				return;
			}
			this.emitError(errorMessage(error));
			this.emitStopped(this.accumulated);
		} finally {
			this.resolveDone();
		}
	}
}

/**
 * Streaming adapter built on `experimental_streamTranscribe`. Base64 PCM16
 * chunks flow straight from the NDJSON `audio` command into the SDK stream.
 */
export class VercelAiSdkAdapter implements SttAdapter {
	readonly mode: TranscriptionMode = "streaming";

	private readonly streamTranscribe: StreamTranscribeFn;
	private readonly resolveModel: ModelResolver;

	constructor(options: VercelAiSdkAdapterOptions = {}) {
		this.streamTranscribe =
			options.streamTranscribe ?? experimental_streamTranscribe;
		this.resolveModel = options.resolveModel ?? defaultModelResolver;
	}

	start(config: SttSessionConfig, emit: EmitFn): SttSession {
		const bridge = new AudioStreamBridge();

		let result: StreamTranscriptionResult;
		try {
			const model = this.resolveModel(config);
			result = this.streamTranscribe({
				model,
				audio: bridge.readable,
				inputAudioFormat: {
					type: "audio/pcm",
					rate: config.sampleRate,
				},
				abortSignal: bridge.abortSignal,
			});
		} catch (error) {
			emit({
				type: "error",
				sessionId: config.sessionId,
				code: "stream_failed",
				message: errorMessage(error),
			});
			return new FailedStreamingSession(config.sessionId, emit);
		}

		emit({ type: "started", sessionId: config.sessionId });
		return new VercelAiSdkSession(
			config.sessionId,
			emit,
			bridge,
			result,
		);
	}
}
