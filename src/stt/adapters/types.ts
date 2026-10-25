import type { OutboundEvent, StartCommand } from "../protocol";
import type { TranscriptionMode } from "../registry";

export type EmitFn = (event: OutboundEvent) => void;

/** Fully resolved session configuration (API key already validated). */
export interface SttSessionConfig {
	readonly sessionId: string;
	readonly provider: string;
	readonly model: string;
	readonly sampleRate: number;
	readonly apiKey: string;
}

/**
 * One live transcription session. The worker forwards every command to the
 * active session and awaits `stop()` before accepting a new `start`.
 */
export interface SttSession {
	readonly sessionId: string;

	/** Enqueues a base64 PCM16 chunk, honoring the pause gate. */
	pushAudio(pcm16Base64: string): void;

	pause(): void;
	resume(): void;

	/** Closes the input and resolves once `stopped` has been emitted. */
	stop(): Promise<void>;

	/** Abrupt teardown (e.g. `stdin` EOF); emits no transcript events. */
	abort(reason?: unknown): Promise<void>;
}

export interface SttAdapter {
	readonly mode: TranscriptionMode;
	start(config: SttSessionConfig, emit: EmitFn): SttSession | Promise<SttSession>;
}

export type SttAdapterFactory = (
	config: SttSessionConfig,
) => SttAdapter | Promise<SttAdapter>;

/** Builds the adapter config from a validated `start` command. */
export function sessionConfigFromStart(
	command: StartCommand,
	apiKey: string,
): SttSessionConfig {
	return {
		sessionId: command.sessionId,
		provider: command.provider,
		model: command.model,
		sampleRate: command.sampleRate,
		apiKey,
	};
}
