/**
 * Provider and model capability registry.
 *
 * Streaming models use the AI SDK `experimental_streamTranscribe` API (live
 * WebSocket transport). Segmented models use the batch `transcribe` API and
 * need a WAV-framed buffer per request.
 */

export type TranscriptionMode = "streaming" | "segmented";

export type Env = Record<string, string | undefined>;

export interface ProviderDefinition {
	/** Canonical provider id used in `start.provider`. */
	readonly id: string;
	/** Environment variables checked in order for an API key. */
	readonly envVars: readonly string[];
	/** Mode used when the model id is not in `MODEL_CAPABILITIES`. */
	readonly defaultMode: TranscriptionMode;
}

export interface ModelCapability {
	readonly provider: string;
	readonly model: string;
	readonly mode: TranscriptionMode;
}

export const PROVIDERS: Readonly<Record<string, ProviderDefinition>> = {
	vercel: {
		id: "vercel",
		envVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
		defaultMode: "streaming",
	},
	gateway: {
		id: "gateway",
		envVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
		defaultMode: "streaming",
	},
	mistral: {
		id: "mistral",
		envVars: ["MISTRAL_API_KEY"],
		defaultMode: "segmented",
	},
	openai: {
		id: "openai",
		envVars: ["OPENAI_API_KEY"],
		defaultMode: "segmented",
	},
	xai: {
		id: "xai",
		envVars: ["XAI_API_KEY"],
		defaultMode: "streaming",
	},
	elevenlabs: {
		id: "elevenlabs",
		envVars: ["ELEVENLABS_API_KEY"],
		defaultMode: "streaming",
	},
};

/**
 * Explicit model capabilities. Keys are the model ids accepted by the AI SDK:
 * either a gateway id (`provider/model`) or the provider-local id.
 */
export const MODEL_CAPABILITIES: Readonly<
	Record<string, TranscriptionMode>
> = {
	// OpenAI
	"openai/gpt-realtime-whisper": "streaming",
	"gpt-realtime-whisper": "streaming",
	"openai/gpt-4o-transcribe": "segmented",
	"gpt-4o-transcribe": "segmented",
	"openai/whisper-1": "segmented",
	"whisper-1": "segmented",

	// xAI
	"xai/grok-stt": "streaming",
	"grok-stt": "streaming",

	// ElevenLabs
	"elevenlabs/eleven-scribe-2-realtime": "streaming",
	"eleven-scribe-2-realtime": "streaming",
	"elevenlabs/scribe_v2_realtime": "streaming",
	"scribe_v2_realtime": "streaming",

	// Mistral Voxtral
	"mistral/voxtral-mini-transcribe-3-0": "segmented",
	"voxtral-mini-transcribe-3-0": "segmented",
	"mistral/voxtral-small-transcribe-3-0": "segmented",
	"voxtral-small-transcribe-3-0": "segmented",
};

function currentEnv(): Env {
	return typeof process === "undefined" ? {} : process.env;
}

export function getProvider(
	providerId: string,
): ProviderDefinition | undefined {
	return Object.hasOwn(PROVIDERS, providerId)
		? PROVIDERS[providerId]
		: undefined;
}

export function providerEnvVars(providerId: string): readonly string[] {
	return getProvider(providerId)?.envVars ?? [];
}

function firstConfigured(
	values: readonly (string | undefined)[],
): string | undefined {
	for (const value of values) {
		if (typeof value === "string" && value.trim().length > 0) {
			return value;
		}
	}
	return undefined;
}

/**
 * Resolves the API key for a provider. An explicit key from the `start`
 * command wins; otherwise the provider environment variables are checked in
 * order.
 */
export function resolveProviderApiKey(
	providerId: string,
	explicitApiKey?: string,
	env: Env = currentEnv(),
): string | undefined {
	const provider = getProvider(providerId);
	if (provider === undefined) {
		return undefined;
	}

	const explicit = firstConfigured([explicitApiKey]);
	if (explicit !== undefined) {
		return explicit;
	}

	return firstConfigured(provider.envVars.map((name) => env[name]));
}

/** A provider is ready when an explicit key or one of its env vars is set. */
export function isProviderAvailable(
	providerId: string,
	explicitApiKey?: string,
	env: Env = currentEnv(),
): boolean {
	return resolveProviderApiKey(providerId, explicitApiKey, env) !== undefined;
}

/**
 * Resolves the transcription mode for a model. Unknown models fall back to the
 * provider default; unknown providers return `undefined`.
 */
export function resolveModelCapability(
	providerId: string,
	model: string,
): ModelCapability | undefined {
	const provider = getProvider(providerId);
	if (provider === undefined) {
		return undefined;
	}

	const explicitMode = Object.hasOwn(MODEL_CAPABILITIES, model)
		? MODEL_CAPABILITIES[model]
		: undefined;

	return {
		provider: providerId,
		model,
		mode: explicitMode ?? provider.defaultMode,
	};
}
