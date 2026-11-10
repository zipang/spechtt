import { describe, expect, test } from "bun:test";
import {
	FALLBACK_LANGUAGE_MODEL,
	LANGUAGE_MODEL_ENV,
	MODEL_CAPABILITIES,
	PROVIDERS,
	PROVIDER_ALIASES,
	defaultLanguageModel,
	getProvider,
	isProviderAvailable,
	listCapabilities,
	providerEnvVars,
	resolveModelCapability,
	resolveProviderApiKey
} from "./registry";
import { LANGUAGE_OPTIONS } from "./languages";

const GATEWAY = "vercel-ai-gateway";

const LIVE_TRANSCRIPTION_MODELS = [
	"elevenlabs/scribe-v2",
	"elevenlabs/scribe-v2-realtime",
	"fish-audio/transcribe-1",
	"google/gemini-3.5-transcribe",
	"google/gemini-3.5-transcribe-live",
	"microsoft/mai-transcribe-2",
	"microsoft/mai-transcribe-2-streaming",
	"mistral/voxtral-mini-transcribe-3",
	"mistral/voxtral-small-transcribe-3",
	"openai/gpt-4o-mini-transcribe",
	"openai/gpt-4o-transcribe",
	"openai/gpt-live-transcribe",
	"openai/gpt-realtime-whisper",
	"openai/gpt-transcribe",
	"openai/whisper-1",
	"spacexai/grok-stt"
];

const STREAMING_MODELS = [
	"elevenlabs/scribe-v2-realtime",
	"google/gemini-3.5-transcribe-live",
	"microsoft/mai-transcribe-2-streaming",
	"openai/gpt-live-transcribe",
	"openai/gpt-realtime-whisper",
	"openai/gpt-transcribe",
	"spacexai/grok-stt"
];

function gatewayCatalogue() {
	const gateway = listCapabilities({ env: {} }).providers.find(
		(provider) => provider.id === GATEWAY
	);
	expect(gateway).toBeDefined();
	return gateway!;
}

describe("provider registry", () => {
	test("offers the Vercel AI Gateway and nothing else", () => {
		expect(getProvider(GATEWAY)?.id).toBe(GATEWAY);
		expect(Object.keys(PROVIDERS)).toEqual([GATEWAY]);
		expect(getProvider("unknown-provider")).toBeUndefined();
		expect(getProvider("")).toBeUndefined();
	});

	test("keeps the direct providers out until their API is evaluated", () => {
		for (const removed of ["mistral", "openai", "xai", "elevenlabs"]) {
			expect(getProvider(removed)).toBeUndefined();
			expect(providerEnvVars(removed)).toEqual([]);
			expect(isProviderAvailable(removed, "sk-explicit", { MISTRAL_API_KEY: "k" })).toBe(
				false
			);
		}
	});

	test("resolves legacy gateway aliases to the canonical provider", () => {
		expect(PROVIDER_ALIASES).toEqual({
			vercel: GATEWAY,
			gateway: GATEWAY
		});
		expect(getProvider("vercel")?.id).toBe(GATEWAY);
		expect(getProvider("gateway")?.id).toBe(GATEWAY);
		expect(Object.keys(PROVIDERS)).not.toContain("vercel");
		expect(Object.keys(PROVIDERS)).not.toContain("gateway");
	});

	test("maps the provider to its environment variables", () => {
		expect(providerEnvVars(GATEWAY)).toEqual([
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY"
		]);
		expect(providerEnvVars("vercel")).toEqual([
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY"
		]);
		expect(providerEnvVars("unknown-provider")).toEqual([]);
	});

	test("no longer needs a per-provider default mode", () => {
		for (const provider of Object.values(PROVIDERS)) {
			expect("defaultMode" in provider).toBe(false);
		}
	});
});

describe("provider availability", () => {
	test("is available when an explicit API key is supplied", () => {
		expect(isProviderAvailable(GATEWAY, "sk-explicit", {})).toBe(true);
		expect(isProviderAvailable("vercel", "sk-explicit", {})).toBe(true);
		expect(isProviderAvailable("mistral", "sk-explicit", {})).toBe(false);
	});

	test("accepts both gateway environment variables through any alias", () => {
		for (const provider of [GATEWAY, "vercel", "gateway"]) {
			expect(
				isProviderAvailable(provider, undefined, {
					AI_GATEWAY_API_KEY: "gateway-key"
				})
			).toBe(true);
			expect(
				isProviderAvailable(provider, undefined, {
					VERCEL_AI_GATEWAY_API_KEY: "gateway-key"
				})
			).toBe(true);
		}
	});

	test("is unavailable without an explicit or environment key", () => {
		expect(isProviderAvailable(GATEWAY, undefined, {})).toBe(false);
		expect(isProviderAvailable(GATEWAY, "", {})).toBe(false);
		expect(
			isProviderAvailable(GATEWAY, undefined, {
				AI_GATEWAY_API_KEY: "   ",
				VERCEL_AI_GATEWAY_API_KEY: ""
			})
		).toBe(false);
		expect(isProviderAvailable("unknown-provider", "sk-explicit", {})).toBe(false);
	});

	test("ignores unrelated provider keys", () => {
		expect(isProviderAvailable(GATEWAY, undefined, { MISTRAL_API_KEY: "mistral" })).toBe(false);
	});
});

describe("resolveProviderApiKey", () => {
	test("prefers the explicit key over the environment", () => {
		expect(
			resolveProviderApiKey(GATEWAY, "sk-explicit", {
				AI_GATEWAY_API_KEY: "env-key"
			})
		).toBe("sk-explicit");
	});

	test("falls back through the provider environment variables in order", () => {
		expect(
			resolveProviderApiKey("vercel", undefined, {
				AI_GATEWAY_API_KEY: "first",
				VERCEL_AI_GATEWAY_API_KEY: "second"
			})
		).toBe("first");

		expect(
			resolveProviderApiKey(GATEWAY, undefined, {
				AI_GATEWAY_API_KEY: "",
				VERCEL_AI_GATEWAY_API_KEY: "second"
			})
		).toBe("second");
	});

	test("returns undefined when nothing is configured", () => {
		expect(resolveProviderApiKey(GATEWAY, undefined, {})).toBeUndefined();
		expect(resolveProviderApiKey("unknown", "key", {})).toBeUndefined();
	});
});

describe("model capability lookup", () => {
	test("marks live transcription models as streaming", () => {
		for (const model of STREAMING_MODELS) {
			expect(resolveModelCapability(GATEWAY, model)).toMatchObject({
				provider: GATEWAY,
				model,
				mode: "streaming"
			});
		}
	});

	test("marks batch transcription models as segmented", () => {
		for (const model of LIVE_TRANSCRIPTION_MODELS.filter(
			(candidate) => !STREAMING_MODELS.includes(candidate)
		)) {
			expect(resolveModelCapability(GATEWAY, model)).toMatchObject({
				provider: GATEWAY,
				model,
				mode: "segmented"
			});
		}
	});

	test("treats unknown models as segmented", () => {
		expect(resolveModelCapability(GATEWAY, "acme/brand-new-live")).toMatchObject({
			mode: "segmented"
		});
	});

	test("returns undefined for unknown providers", () => {
		expect(resolveModelCapability("mistral", "voxtral-mini-transcribe-3")).toBeUndefined();
		expect(resolveModelCapability("unknown", "some-model")).toBeUndefined();
	});
});

describe("capability listing", () => {
	test("lists the single provider deterministically", () => {
		const first = listCapabilities({ env: {} }).providers;
		const second = listCapabilities({ env: {} }).providers;

		expect(first).toEqual(second);
		expect(first.map((provider) => provider.id)).toEqual([GATEWAY]);
		expect(first[0]?.label).toBe("Vercel AI Gateway");
		expect(first[0]?.models.length).toBe(16);

		const modelLabels = first[0]?.models.map((model) => model.label) ?? [];
		expect(modelLabels).toEqual([...modelLabels].sort());
		expect(new Set(first[0]?.models.map((model) => model.id)).size).toBe(16);
	});

	test("emits the provider key environment variables on the wire", () => {
		expect(
			listCapabilities({ env: {} }).providers.find((provider) => provider.id === GATEWAY)
				?.keyEnvVars
		).toEqual(["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"]);
	});

	test("every listed model has an explicit, valid transcription mode", () => {
		for (const provider of listCapabilities({ env: {} }).providers) {
			for (const model of provider.models) {
				expect(["streaming", "segmented"]).toContain(model.mode);
				expect(MODEL_CAPABILITIES[model.id]).toBe(model.mode);
				expect(model.label.length).toBeGreaterThan(0);
			}
		}
	});

	test("lists the live gateway transcription set with the 7/9 mode split", () => {
		const gateway = gatewayCatalogue();
		const ids = gateway.models.map((model) => model.id);

		// Sorted by label (`name` on the live endpoint), then id.
		expect(ids).toEqual([
			"openai/gpt-live-transcribe",
			"openai/gpt-transcribe",
			"openai/gpt-4o-transcribe",
			"openai/gpt-4o-mini-transcribe",
			"google/gemini-3.5-transcribe",
			"google/gemini-3.5-transcribe-live",
			"spacexai/grok-stt",
			"microsoft/mai-transcribe-2",
			"microsoft/mai-transcribe-2-streaming",
			"elevenlabs/scribe-v2",
			"elevenlabs/scribe-v2-realtime",
			"fish-audio/transcribe-1",
			"mistral/voxtral-mini-transcribe-3",
			"mistral/voxtral-small-transcribe-3",
			"openai/whisper-1",
			"openai/gpt-realtime-whisper"
		]);
		expect([...ids].sort()).toEqual([...LIVE_TRANSCRIPTION_MODELS].sort());
		expect(
			gateway.models
				.filter((model) => model.mode === "streaming")
				.map((model) => model.id)
				.sort()
		).toEqual([...STREAMING_MODELS].sort());
	});

	test("uses the live endpoint names as gateway model labels", () => {
		const labels = new Map(gatewayCatalogue().models.map((model) => [model.id, model.label]));

		expect(labels.get("openai/gpt-realtime-whisper")).toBe("gpt-realtime-whisper");
		expect(labels.get("openai/whisper-1")).toBe("Whisper");
		expect(labels.get("mistral/voxtral-mini-transcribe-3")).toBe("Voxtral Mini Transcribe 3");
		expect(labels.get("openai/gpt-4o-mini-transcribe")).toBe("GPT-4o mini Transcribe");
	});

	test("curated models carry no description or pricing", () => {
		for (const model of gatewayCatalogue().models) {
			expect(model.description).toBeNull();
			expect(model.pricing).toEqual({});
		}
	});

	test("drops the non-existent eleven-scribe-2-realtime id everywhere", () => {
		const serialized = JSON.stringify(listCapabilities({ env: {} }));

		expect(serialized).not.toContain("eleven-scribe-2-realtime");
		expect(MODEL_CAPABILITIES).not.toHaveProperty("eleven-scribe-2-realtime");
		expect(MODEL_CAPABILITIES).not.toHaveProperty("elevenlabs/eleven-scribe-2-realtime");
	});

	test("passes discovered metadata through and keeps the curated catalogue otherwise", () => {
		const providers = listCapabilities({
			env: {},
			gatewayModels: [
				{
					id: "acme/new-live-model",
					label: "New Live Model",
					mode: "streaming",
					description: "Acme live transcription.",
					pricing: { transcription_duration_cost_per_second: "0.00004" }
				},
				{
					id: "acme/new-batch-model",
					label: "New Batch Model",
					mode: "segmented"
				}
			]
		}).providers;
		const gateway = providers.find((provider) => provider.id === GATEWAY)!;

		expect(gateway.models).toEqual([
			{
				id: "acme/new-batch-model",
				label: "New Batch Model",
				mode: "segmented",
				description: null,
				pricing: {}
			},
			{
				id: "acme/new-live-model",
				label: "New Live Model",
				mode: "streaming",
				description: "Acme live transcription.",
				pricing: { transcription_duration_cost_per_second: "0.00004" }
			}
		]);
		expect(providers).toHaveLength(1);
	});

	test("ignores an empty discovered catalogue", () => {
		expect(listCapabilities({ env: {}, gatewayModels: [] }).providers[0]?.models).toHaveLength(
			16
		);
	});

	test("availability reflects the gateway environment", () => {
		const availableIds = (env: Record<string, string | undefined>) =>
			listCapabilities({ env })
				.providers.filter((provider) => provider.available)
				.map((provider) => provider.id);

		expect(availableIds({})).toEqual([]);
		expect(availableIds({ AI_GATEWAY_API_KEY: "gateway-key" })).toEqual([GATEWAY]);
		expect(availableIds({ MISTRAL_API_KEY: "env-key" })).toEqual([]);
	});

	test("a supplied key marks the provider available", () => {
		expect(
			listCapabilities({
				env: {},
				apiKeyFor: () => "sk-explicit"
			}).providers[0]?.available
		).toBe(true);
		expect(
			listCapabilities({ env: {}, apiKeyFor: () => undefined }).providers[0]?.available
		).toBe(false);
	});

	test("never exposes key values in the capability payload", () => {
		const payload = JSON.stringify(
			listCapabilities({
				env: { AI_GATEWAY_API_KEY: "sk-env-secret" },
				apiKeyFor: () => "sk-explicit-secret"
			})
		);

		expect(payload).not.toContain("sk-env-secret");
		expect(payload).not.toContain("sk-explicit-secret");
	});
});

describe("cleanup model defaults", () => {
	test("falls back to the built-in model without an override", () => {
		expect(defaultLanguageModel({})).toBe(FALLBACK_LANGUAGE_MODEL);
		expect(FALLBACK_LANGUAGE_MODEL).toBe("openai/gpt-4.1-mini");
		expect(LANGUAGE_MODEL_ENV).toBe("DEFAULT_LANGUAGE_MODEL");
	});

	test("prefers the environment override", () => {
		expect(
			defaultLanguageModel({
				[LANGUAGE_MODEL_ENV]: "anthropic/claude-sonnet-5.5"
			})
		).toBe("anthropic/claude-sonnet-5.5");
		expect(defaultLanguageModel({ [LANGUAGE_MODEL_ENV]: "  openai/gpt-4.1-mini  " })).toBe(
			"openai/gpt-4.1-mini"
		);
	});

	test("ignores an empty or blank override", () => {
		expect(defaultLanguageModel({ [LANGUAGE_MODEL_ENV]: "" })).toBe(FALLBACK_LANGUAGE_MODEL);
		expect(defaultLanguageModel({ [LANGUAGE_MODEL_ENV]: "   " })).toBe(FALLBACK_LANGUAGE_MODEL);
	});
});

describe("cleanup and language capabilities", () => {
	test("reports the offered dictation languages", () => {
		const payload = listCapabilities({ env: {} });

		expect(payload.languages).toEqual(LANGUAGE_OPTIONS);
		expect(JSON.parse(JSON.stringify(payload.languages))).toEqual([...LANGUAGE_OPTIONS]);
	});

	test("reports the resolved default cleanup model", () => {
		expect(listCapabilities({ env: {} }).defaultLanguageModel).toBe(FALLBACK_LANGUAGE_MODEL);
		expect(
			listCapabilities({
				env: { [LANGUAGE_MODEL_ENV]: "openai/gpt-4.1" }
			}).defaultLanguageModel
		).toBe("openai/gpt-4.1");
	});

	test("reports null language models when discovery returned none", () => {
		expect(listCapabilities({ env: {} }).languageModels).toBeNull();
		expect(listCapabilities({ env: {}, languageModels: [] }).languageModels).toBeNull();
	});

	test("passes discovered language models through unchanged", () => {
		const languageModels = [
			{ id: "anthropic/claude-sonnet-5.5", label: "Claude Sonnet 5.5" },
			{ id: "openai/gpt-4.1-mini", label: "GPT-4.1 mini" }
		];

		expect(listCapabilities({ env: {}, languageModels }).languageModels).toEqual([
			{ id: "anthropic/claude-sonnet-5.5", label: "Claude Sonnet 5.5" },
			{ id: "openai/gpt-4.1-mini", label: "GPT-4.1 mini" }
		]);
	});

	test("keeps the provider list unchanged", () => {
		const payload = listCapabilities({ env: {} });

		expect(payload.providers.map((provider) => provider.id)).toEqual([GATEWAY]);
		expect(payload.providers[0]?.models).toHaveLength(16);
	});

	test("never leaks a key through the new fields", () => {
		const payload = JSON.stringify(
			listCapabilities({
				env: { AI_GATEWAY_API_KEY: "sk-env-secret" },
				apiKeyFor: () => "sk-explicit-secret",
				languageModels: [{ id: "openai/gpt-4.1-mini", label: "GPT-4.1 mini" }]
			})
		);

		expect(payload).not.toContain("sk-env-secret");
		expect(payload).not.toContain("sk-explicit-secret");
	});
});
