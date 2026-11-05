/**
 * Contracts shared by the transcription adapters.
 *
 * A session config is built once from a validated `start` command by
 * [`sessionConfigFromStart`] and handed to `VercelAiSdkAdapter`,
 * `MistralAdapter` or `createFileSession` (`adapters/file.ts`). The provider
 * options a session sends are assembled here with
 * [`mergeProviderOptions`], so the dictation language never replaces an
 * adapter's own settings.
 */

import type { transcribe } from "ai";
import type { OutboundEvent, StartCommand } from "../protocol";
import type { TranscriptionMode } from "../registry";

export type EmitFn = (event: OutboundEvent) => void;

/**
 * Provider options exactly as the AI SDK documents them: keyed by provider
 * package, values are that package's option object.
 */
export type ProviderOptions = NonNullable<
	Parameters<typeof transcribe>[0]["providerOptions"]
>;

/** Fully resolved session configuration (API key already validated). */
export interface SttSessionConfig {
	readonly sessionId: string;
	readonly provider: string;
	readonly model: string;
	readonly sampleRate: number;
	readonly apiKey: string;
	/**
	 * ISO-639-1 language of the dictation. Absent leaves detection to the
	 * provider; both adapters forward it through `languageProviderOptions`.
	 */
	readonly language?: string;
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
		...(command.language === undefined ? {} : { language: command.language }),
	};
}

/**
 * Merges two provider-option maps, one provider package at a time.
 *
 * Both adapters already hold options of their own and add the dictation
 * language on top, so a shallow spread would drop the outer package's existing
 * keys whenever the language targets the same package. Either side may be
 * absent, and the result is `undefined` when neither has options, so an
 * untouched session sends exactly the payload it sent before.
 */
export function mergeProviderOptions(
	base: ProviderOptions | undefined,
	extra: ProviderOptions | undefined,
): ProviderOptions | undefined {
	if (base === undefined) {
		return extra;
	}
	if (extra === undefined) {
		return base;
	}

	const merged: ProviderOptions = { ...base };
	for (const [provider, options] of Object.entries(extra)) {
		merged[provider] = { ...base[provider], ...options };
	}
	return merged;
}
