/**
 * Provider and model capability registry.
 *
 * Streaming models use the AI SDK `experimental_streamTranscribe` API (live
 * WebSocket transport). Segmented models use the batch `transcribe` API and
 * need a WAV-framed buffer per request.
 *
 * The curated catalogue below is the offline fallback: the worker prefers the
 * live gateway catalogue discovered at runtime (see `discovery.ts`).
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
	/** Models offered by this provider, in catalogue order. */
	readonly models: readonly ModelDefinition[];
}

/** One selectable model and how it consumes audio. */
export interface ModelDefinition {
	/** Model id accepted by the AI SDK for this provider. */
	readonly id: string;
	/** Human-readable name shown in Settings. */
	readonly label: string;
	readonly mode: TranscriptionMode;
	/** Static fallback metadata; live discovery usually supersedes it. */
	readonly description?: string | null;
	readonly pricing?: Readonly<Record<string, string>>;
}

export interface ModelCapabilityInfo {
	readonly id: string;
	readonly label: string;
	readonly mode: TranscriptionMode;
	/** Gateway description, when it publishes one. */
	readonly description: string | null;
	/** Raw gateway pricing map (values as strings), empty when unpublished. */
	readonly pricing: Readonly<Record<string, string>>;
}

export interface ProviderCapabilityInfo {
	readonly id: string;
	readonly label: string;
	readonly available: boolean;
	/** Environment variables the UI names in the API-key row. */
	readonly keyEnvVars: readonly string[];
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
	/**
	 * Live gateway catalogue from discovery. When non-empty it replaces the
	 * curated gateway models; direct providers keep their curated catalogues.
	 */
	readonly gatewayModels?: readonly ModelDefinition[];
}

/** Canonical id of the Vercel AI Gateway provider. */
export const GATEWAY_PROVIDER_ID = "vercel-ai-gateway";

/**
 * Legacy ids accepted from stored settings and older `start` commands.
 * They always resolve to the canonical provider and are never listed.
 */
export const PROVIDER_ALIASES: Readonly<Record<string, string>> = {
	vercel: GATEWAY_PROVIDER_ID,
	gateway: GATEWAY_PROVIDER_ID,
};

/** Models reachable through the Vercel AI Gateway (`provider/model` ids). */
const GATEWAY_MODELS: readonly ModelDefinition[] = [
	{ id: "elevenlabs/scribe-v2", label: "Scribe v2", mode: "segmented" },
	{
		id: "elevenlabs/scribe-v2-realtime",
		label: "Scribe v2 Realtime",
		mode: "streaming",
	},
	{ id: "fish-audio/transcribe-1", label: "Transcribe-1", mode: "segmented" },
	{
		id: "google/gemini-3.5-transcribe",
		label: "Gemini 3.5 Transcribe",
		mode: "segmented",
	},
	{
		id: "google/gemini-3.5-transcribe-live",
		label: "Gemini 3.5 Transcribe Live",
		mode: "streaming",
	},
	{
		id: "microsoft/mai-transcribe-2",
		label: "MAI-Transcribe 2",
		mode: "segmented",
	},
	{
		id: "microsoft/mai-transcribe-2-streaming",
		label: "MAI-Transcribe-2-Streaming",
		mode: "streaming",
	},
	{
		id: "mistral/voxtral-mini-transcribe-3",
		label: "Voxtral Mini Transcribe 3",
		mode: "segmented",
	},
	{
		id: "mistral/voxtral-small-transcribe-3",
		label: "Voxtral Small Transcribe 3",
		mode: "segmented",
	},
	{
		id: "openai/gpt-4o-mini-transcribe",
		label: "GPT-4o mini Transcribe",
		mode: "segmented",
	},
	{
		id: "openai/gpt-4o-transcribe",
		label: "GPT-4o Transcribe",
		mode: "segmented",
	},
	{
		id: "openai/gpt-live-transcribe",
		label: "GPT Live Transcribe",
		mode: "streaming",
	},
	{
		id: "openai/gpt-realtime-whisper",
		label: "gpt-realtime-whisper",
		mode: "streaming",
	},
	{ id: "openai/gpt-transcribe", label: "GPT Transcribe", mode: "streaming" },
	{ id: "openai/whisper-1", label: "Whisper", mode: "segmented" },
	{ id: "spacexai/grok-stt", label: "Grok STT", mode: "streaming" },
];

export const PROVIDERS: Readonly<Record<string, ProviderDefinition>> = {
	[GATEWAY_PROVIDER_ID]: {
		id: GATEWAY_PROVIDER_ID,
		label: "Vercel AI Gateway",
		envVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
		models: GATEWAY_MODELS,
	},
};

/**
 * Explicit mode per model id, derived from the curated catalogues so the two
 * can never drift. Unknown ids are treated as batch by
 * [`resolveModelCapability`].
 */
export const MODEL_CAPABILITIES: Readonly<Record<string, TranscriptionMode>> =
	Object.fromEntries(
		Object.values(PROVIDERS).flatMap((provider) =>
			provider.models.map((model) => [model.id, model.mode] as const),
		),
	);

export interface ModelCapability {
	readonly provider: string;
	readonly model: string;
	readonly mode: TranscriptionMode;
}

function currentEnv(): Env {
	return typeof process === "undefined" ? {} : process.env;
}

/** Resolves a canonical or legacy provider id to its definition. */
export function getProvider(
	providerId: string,
): ProviderDefinition | undefined {
	const canonical = PROVIDER_ALIASES[providerId] ?? providerId;
	return Object.hasOwn(PROVIDERS, canonical)
		? PROVIDERS[canonical]
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
 * Resolves how a model consumes audio.
 *
 * Every curated id has an explicit mode; unknown ids fall back to `segmented`
 * because batch transcription is the broader endpoint, and a wrong batch call
 * fails cleanly instead of opening a doomed WebSocket.
 */
export function resolveModelCapability(
	providerId: string,
	model: string,
): ModelCapability | undefined {
	const provider = getProvider(providerId);
	if (provider === undefined) {
		return undefined;
	}

	return {
		provider: provider.id,
		model,
		mode: MODEL_CAPABILITIES[model] ?? "segmented",
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
	const discovered =
		options.gatewayModels !== undefined && options.gatewayModels.length > 0
			? options.gatewayModels
			: undefined;

	return Object.values(PROVIDERS)
		.map((provider): ProviderCapabilityInfo => {
			const explicitKey = options.apiKeyFor?.(provider.id);
			const usingDiscovered =
				provider.id === GATEWAY_PROVIDER_ID && discovered !== undefined;
			const models = usingDiscovered ? discovered : provider.models;

			return {
				id: provider.id,
				label: provider.label,
				available: isProviderAvailable(
					provider.id,
					explicitKey,
					options.env,
				),
				keyEnvVars: provider.envVars,
				models: models
					.map(
						(model): ModelCapabilityInfo => ({
							id: model.id,
							label: model.label,
							mode: usingDiscovered
								? model.mode
								: (MODEL_CAPABILITIES[model.id] ?? "segmented"),
							description: model.description ?? null,
							pricing: model.pricing ?? {},
						}),
					)
					.sort(compareByLabel),
			};
		})
		.sort(compareByLabel);
}
