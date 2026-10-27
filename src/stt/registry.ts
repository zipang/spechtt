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
	/** Human-readable name shown in Settings. */
	readonly label: string;
	/** Environment variables checked in order for an API key. */
	readonly envVars: readonly string[];
	/** Mode used when the model id is not in `MODEL_CAPABILITIES`. */
	readonly defaultMode: TranscriptionMode;
	/** Models offered by this provider, in catalogue order. */
	readonly models: readonly ModelDefinition[];
}

/** One selectable model; its mode always comes from `MODEL_CAPABILITIES`. */
export interface ModelDefinition {
	/** Model id accepted by the AI SDK for this provider. */
	readonly id: string;
	/** Human-readable name shown in Settings. */
	readonly label: string;
}

export interface ModelCapabilityInfo {
	readonly id: string;
	readonly label: string;
	readonly mode: TranscriptionMode;
}

export interface ProviderCapabilityInfo {
	readonly id: string;
	readonly label: string;
	readonly available: boolean;
	readonly models: readonly ModelCapabilityInfo[];
}

export interface ListCapabilitiesOptions {
	/**
	 * Explicit key for a provider, e.g. one typed in Settings but not exported
	 * to the environment yet. Never returned in the payload.
	 */
	readonly apiKeyFor?: (providerId: string) => string | undefined;
	/** Environment to consult; defaults to `process.env`. */
	readonly env?: Env;
}

/** Models reachable through the Vercel AI Gateway (`provider/model` ids). */
const GATEWAY_MODELS: readonly ModelDefinition[] = [
	{ id: "fish-audio/transcribe-1", label: "Transcribe 1" },
	{ id: "google/gemini-3.5-transcribe", label: "Gemini 3.5 Transcribe" },
	{
		id: "google/gemini-3.5-transcribe-live",
		label: "Gemini 3.5 Transcribe Live",
	},
	{ id: "microsoft/mai-transcribe-2", label: "MAI-Transcribe 2" },
	{
		id: "microsoft/mai-transcribe-2-streaming",
		label: "MAI-Transcribe 2 Streaming",
	},
	{
		id: "mistral/voxtral-mini-transcribe-3",
		label: "Voxtral Mini Transcribe 3",
	},
	{
		id: "mistral/voxtral-small-transcribe-3",
		label: "Voxtral Small Transcribe 3",
	},
	{
		id: "openai/gpt-4o-mini-transcribe",
		label: "GPT-4o Mini Transcribe",
	},
	{ id: "openai/gpt-4o-transcribe", label: "GPT-4o Transcribe" },
	{ id: "openai/gpt-realtime-whisper", label: "GPT Realtime Whisper" },
	{ id: "openai/whisper-1", label: "Whisper 1" },
	{ id: "spacexai/grok-stt", label: "Grok STT" },
];

export const PROVIDERS: Readonly<Record<string, ProviderDefinition>> = {
	vercel: {
		id: "vercel",
		label: "Vercel AI Gateway",
		envVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
		defaultMode: "streaming",
		models: GATEWAY_MODELS,
	},
	gateway: {
		id: "gateway",
		label: "Vercel AI Gateway (alias)",
		envVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
		defaultMode: "streaming",
		models: GATEWAY_MODELS,
	},
	mistral: {
		id: "mistral",
		label: "Mistral AI",
		envVars: ["MISTRAL_API_KEY"],
		defaultMode: "segmented",
		models: [
			{
				id: "voxtral-mini-transcribe-3-0",
				label: "Voxtral Mini Transcribe 3.0",
			},
			{
				id: "voxtral-small-transcribe-3-0",
				label: "Voxtral Small Transcribe 3.0",
			},
		],
	},
	openai: {
		id: "openai",
		label: "OpenAI",
		envVars: ["OPENAI_API_KEY"],
		defaultMode: "segmented",
		models: [
			{ id: "gpt-realtime-whisper", label: "GPT Realtime Whisper" },
			{ id: "gpt-4o-transcribe", label: "GPT-4o Transcribe" },
			{ id: "whisper-1", label: "Whisper 1" },
		],
	},
	xai: {
		id: "xai",
		label: "xAI",
		envVars: ["XAI_API_KEY"],
		defaultMode: "streaming",
		models: [{ id: "grok-stt", label: "Grok STT" }],
	},
	elevenlabs: {
		id: "elevenlabs",
		label: "ElevenLabs",
		envVars: ["ELEVENLABS_API_KEY"],
		defaultMode: "streaming",
		models: [
			{
				id: "eleven-scribe-2-realtime",
				label: "Eleven Scribe 2 Realtime",
			},
			{ id: "scribe_v2_realtime", label: "Scribe v2 Realtime" },
		],
	},
};

export interface ModelCapability {
	readonly provider: string;
	readonly model: string;
	readonly mode: TranscriptionMode;
}

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
	"openai/gpt-4o-mini-transcribe": "segmented",
	"gpt-4o-mini-transcribe": "segmented",
	"openai/whisper-1": "segmented",
	"whisper-1": "segmented",

	// xAI
	"spacexai/grok-stt": "streaming",
	"xai/grok-stt": "streaming",
	"grok-stt": "streaming",

	// Google
	"google/gemini-3.5-transcribe": "segmented",
	"google/gemini-3.5-transcribe-live": "streaming",

	// Microsoft
	"microsoft/mai-transcribe-2": "segmented",
	"microsoft/mai-transcribe-2-streaming": "streaming",

	// Mistral Voxtral (gateway ids)
	"mistral/voxtral-mini-transcribe-3": "segmented",
	"mistral/voxtral-small-transcribe-3": "segmented",

	// Fish Audio
	"fish-audio/transcribe-1": "segmented",

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

/**
 * Deterministic code-unit ordering, independent of the host locale. Sorting by
 * label keeps the Settings dialog stable; the id breaks label ties.
 */
function compareByLabel(
	left: { readonly label: string; readonly id: string },
	right: { readonly label: string; readonly id: string },
): number {
	if (left.label !== right.label) {
		return left.label < right.label ? -1 : 1;
	}
	if (left.id === right.id) {
		return 0;
	}
	return left.id < right.id ? -1 : 1;
}

/**
 * Read-only provider/model catalogue for the Settings dialog.
 *
 * Availability is computed per provider from an explicit key (when supplied)
 * or the environment. Models are always listed, even for unavailable
 * providers, so the UI can explain how to enable them. The returned arrays are
 * sorted by label and never contain key material.
 */
export function listCapabilities(
	options: ListCapabilitiesOptions = {},
): readonly ProviderCapabilityInfo[] {
	return Object.values(PROVIDERS)
		.map((provider): ProviderCapabilityInfo => {
			const explicitKey = options.apiKeyFor?.(provider.id);
			return {
				id: provider.id,
				label: provider.label,
				available: isProviderAvailable(
					provider.id,
					explicitKey,
					options.env,
				),
				models: provider.models
					.map(
						(model): ModelCapabilityInfo => ({
							id: model.id,
							label: model.label,
							mode:
								resolveModelCapability(provider.id, model.id)
									?.mode ?? provider.defaultMode,
						}),
					)
					.sort(compareByLabel),
			};
		})
		.sort(compareByLabel);
}
