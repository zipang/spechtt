import { describe, expect, test } from "bun:test";
import {
	MODEL_CAPABILITIES,
	PROVIDERS,
	PROVIDER_ALIASES,
	getProvider,
	isProviderAvailable,
	listCapabilities,
	providerEnvVars,
	resolveModelCapability,
	resolveProviderApiKey,
} from "./registry";

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
	"spacexai/grok-stt",
];

const STREAMING_MODELS = [
	"elevenlabs/scribe-v2-realtime",
	"google/gemini-3.5-transcribe-live",
	"microsoft/mai-transcribe-2-streaming",
	"openai/gpt-live-transcribe",
	"openai/gpt-realtime-whisper",
	"openai/gpt-transcribe",
	"spacexai/grok-stt",
];

function gatewayCatalogue() {
	const gateway = listCapabilities({ env: {} }).find(
		(provider) => provider.id === GATEWAY,
	);
	expect(gateway).toBeDefined();
	return gateway!;
}

describe("provider registry", () => {
	test("exposes the supported STT providers under canonical ids", () => {
		for (const provider of [
			GATEWAY,
			"mistral",
			"openai",
			"xai",
			"elevenlabs",
		]) {
			expect(getProvider(provider)?.id).toBe(provider);
		}

		expect(getProvider("unknown-provider")).toBeUndefined();
		expect(getProvider("")).toBeUndefined();
		expect(Object.keys(PROVIDERS).length).toBe(5);
	});

	test("resolves legacy gateway aliases to the canonical provider", () => {
		expect(PROVIDER_ALIASES).toEqual({
			vercel: GATEWAY,
			gateway: GATEWAY,
		});
		expect(getProvider("vercel")?.id).toBe(GATEWAY);
		expect(getProvider("gateway")?.id).toBe(GATEWAY);
		expect(
			Object.keys(PROVIDERS).filter((id) => id === "vercel" || id === "gateway"),
		).toEqual([]);
	});

	test("maps providers to their environment variables", () => {
		expect(providerEnvVars(GATEWAY)).toEqual([
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY",
		]);
		expect(providerEnvVars("vercel")).toEqual([
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY",
		]);
		expect(providerEnvVars("gateway")).toEqual([
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY",
		]);
		expect(providerEnvVars("mistral")).toEqual(["MISTRAL_API_KEY"]);
		expect(providerEnvVars("openai")).toEqual(["OPENAI_API_KEY"]);
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
		expect(isProviderAvailable("mistral", "sk-explicit", {})).toBe(true);
		expect(isProviderAvailable("mistral", "sk-explicit")).toBe(true);
	});

	test("is available when the primary environment variable is set", () => {
		expect(
			isProviderAvailable("mistral", undefined, {
				MISTRAL_API_KEY: "env-key",
			}),
		).toBe(true);
		expect(
			isProviderAvailable("openai", undefined, {
				OPENAI_API_KEY: "env-key",
			}),
		).toBe(true);
	});

	test("accepts both gateway environment variables through any alias", () => {
		for (const provider of [GATEWAY, "vercel", "gateway"]) {
			expect(
				isProviderAvailable(provider, undefined, {
					AI_GATEWAY_API_KEY: "gateway-key",
				}),
			).toBe(true);
			expect(
				isProviderAvailable(provider, undefined, {
					VERCEL_AI_GATEWAY_API_KEY: "gateway-key",
				}),
			).toBe(true);
		}
	});

	test("is unavailable without an explicit or environment key", () => {
		expect(isProviderAvailable("mistral", undefined, {})).toBe(false);
		expect(isProviderAvailable("openai", "", {})).toBe(false);
		expect(
			isProviderAvailable(GATEWAY, undefined, {
				AI_GATEWAY_API_KEY: "   ",
				VERCEL_AI_GATEWAY_API_KEY: "",
			}),
		).toBe(false);
		expect(isProviderAvailable("unknown-provider", "sk-explicit", {})).toBe(
			false,
		);
	});

	test("does not leak one provider's key to another", () => {
		const env = { MISTRAL_API_KEY: "mistral-key" };

		expect(isProviderAvailable("mistral", undefined, env)).toBe(true);
		expect(isProviderAvailable("openai", undefined, env)).toBe(false);
	});
});

describe("resolveProviderApiKey", () => {
	test("prefers the explicit key over the environment", () => {
		expect(
			resolveProviderApiKey("mistral", "sk-explicit", {
				MISTRAL_API_KEY: "env-key",
			}),
		).toBe("sk-explicit");
	});

	test("falls back through the provider environment variables in order", () => {
		expect(
			resolveProviderApiKey("vercel", undefined, {
				AI_GATEWAY_API_KEY: "first",
				VERCEL_AI_GATEWAY_API_KEY: "second",
			}),
		).toBe("first");

		expect(
			resolveProviderApiKey(GATEWAY, undefined, {
				AI_GATEWAY_API_KEY: "",
				VERCEL_AI_GATEWAY_API_KEY: "second",
			}),
		).toBe("second");
	});

	test("returns undefined when nothing is configured", () => {
		expect(resolveProviderApiKey("openai", undefined, {})).toBeUndefined();
		expect(resolveProviderApiKey("unknown", "key", {})).toBeUndefined();
	});
});

describe("model capability lookup", () => {
	test("marks live transcription models as streaming", () => {
		for (const model of STREAMING_MODELS) {
			expect(resolveModelCapability(GATEWAY, model)).toMatchObject({
				provider: GATEWAY,
				model,
				mode: "streaming",
			});
		}

		for (const [provider, model] of [
			["openai", "gpt-realtime-whisper"],
			["openai", "gpt-live-transcribe"],
			["openai", "gpt-transcribe"],
			["xai", "grok-stt"],
			["elevenlabs", "scribe_v2_realtime"],
		] as const) {
			expect(resolveModelCapability(provider, model)).toMatchObject({
				provider,
				model,
				mode: "streaming",
			});
		}
	});

	test("marks batch transcription models as segmented", () => {
		for (const model of LIVE_TRANSCRIPTION_MODELS.filter(
			(candidate) => !STREAMING_MODELS.includes(candidate),
		)) {
			expect(resolveModelCapability(GATEWAY, model)).toMatchObject({
				provider: GATEWAY,
				model,
				mode: "segmented",
			});
		}

		for (const [provider, model] of [
			["mistral", "voxtral-mini-transcribe-3-0"],
			["openai", "whisper-1"],
			["openai", "gpt-4o-transcribe"],
			["elevenlabs", "scribe_v2"],
		] as const) {
			expect(resolveModelCapability(provider, model)).toMatchObject({
				provider,
				model,
				mode: "segmented",
			});
		}
	});

	test("treats unknown models as segmented for every provider", () => {
		for (const provider of [GATEWAY, "mistral", "openai", "xai", "elevenlabs"]) {
			expect(
				resolveModelCapability(provider, "acme/brand-new-live"),
			).toMatchObject({ mode: "segmented" });
		}
	});

	test("returns undefined for unknown providers", () => {
		expect(resolveModelCapability("unknown", "some-model")).toBeUndefined();
	});
});

describe("capability listing", () => {
	test("lists every provider once, deterministically sorted by label", () => {
		const first = listCapabilities({ env: {} });
		const second = listCapabilities({ env: {} });

		expect(first).toEqual(second);
		expect(first.map((provider) => provider.id)).toEqual([
			"elevenlabs",
			"mistral",
			"openai",
			GATEWAY,
			"xai",
		]);

		const labels = first.map((provider) => provider.label);
		expect(labels).toEqual([...labels].sort());

		for (const provider of first) {
			expect(provider.id.length).toBeGreaterThan(0);
			expect(provider.label.length).toBeGreaterThan(0);
			expect(provider.models.length).toBeGreaterThan(0);

			const modelLabels = provider.models.map((model) => model.label);
			expect(modelLabels).toEqual([...modelLabels].sort());
			expect(new Set(provider.models.map((model) => model.id)).size).toBe(
				provider.models.length,
			);
		}
	});

	test("emits the provider key environment variables on the wire", () => {
		const providers = listCapabilities({ env: {} });

		expect(
			providers.find((provider) => provider.id === GATEWAY)?.keyEnvVars,
		).toEqual(["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"]);
		expect(
			providers.find((provider) => provider.id === "mistral")?.keyEnvVars,
		).toEqual(["MISTRAL_API_KEY"]);
	});

	test("every listed model has an explicit, valid transcription mode", () => {
		for (const provider of listCapabilities({ env: {} })) {
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
			"openai/gpt-realtime-whisper",
		]);
		expect(ids).toHaveLength(16);
		expect([...ids].sort()).toEqual([...LIVE_TRANSCRIPTION_MODELS].sort());
		expect(
			gateway.models
				.filter((model) => model.mode === "streaming")
				.map((model) => model.id)
				.sort(),
		).toEqual([...STREAMING_MODELS].sort());
	});

	test("uses the live endpoint names as gateway model labels", () => {
		const labels = new Map(
			gatewayCatalogue().models.map((model) => [model.id, model.label]),
		);

		expect(labels.get("openai/gpt-realtime-whisper")).toBe(
			"gpt-realtime-whisper",
		);
		expect(labels.get("openai/whisper-1")).toBe("Whisper");
		expect(labels.get("mistral/voxtral-mini-transcribe-3")).toBe(
			"Voxtral Mini Transcribe 3",
		);
		expect(labels.get("openai/gpt-4o-mini-transcribe")).toBe(
			"GPT-4o mini Transcribe",
		);
	});

	test("keeps a single gateway entry under the canonical id", () => {
		const providers = listCapabilities({ env: {} });
		const gatewayEntries = providers.filter(
			(provider) => provider.id === GATEWAY,
		);

		expect(gatewayEntries).toHaveLength(1);
		expect(providers.map((provider) => provider.id)).not.toContain("vercel");
		expect(providers.map((provider) => provider.id)).not.toContain("gateway");
	});

	test("lists direct-provider catalogues without gateway-qualified ids", () => {
		const providers = listCapabilities({ env: {} });
		const ids = (id: string) =>
			providers
				.find((provider) => provider.id === id)!
				.models.map((model) => model.id);

		expect(ids("openai")).toEqual([
			"gpt-live-transcribe",
			"gpt-realtime-whisper",
			"gpt-transcribe",
			"gpt-4o-transcribe",
			"whisper-1",
		]);
		expect(ids("openai")).not.toContain("openai/gpt-realtime-whisper");
		expect(ids("elevenlabs")).toEqual(["scribe_v2", "scribe_v2_realtime"]);
		expect(ids("mistral")).toEqual([
			"voxtral-mini-transcribe-3-0",
			"voxtral-small-transcribe-3-0",
		]);
		expect(ids("xai")).toEqual(["grok-stt"]);
	});

	test("drops the non-existent eleven-scribe-2-realtime id everywhere", () => {
		const serialized = JSON.stringify(listCapabilities({ env: {} }));

		expect(serialized).not.toContain("eleven-scribe-2-realtime");
		expect(MODEL_CAPABILITIES).not.toHaveProperty(
			"eleven-scribe-2-realtime",
		);
		expect(MODEL_CAPABILITIES).not.toHaveProperty(
			"elevenlabs/eleven-scribe-2-realtime",
		);
	});

	test("replaces only the gateway catalogue when discovered models are given", () => {
		const providers = listCapabilities({
			env: {},
			gatewayModels: [
				{
					id: "acme/new-live-model",
					label: "New Live Model",
					mode: "streaming",
				},
				{
					id: "acme/new-batch-model",
					label: "New Batch Model",
					mode: "segmented",
				},
			],
		});
		const gateway = providers.find((provider) => provider.id === GATEWAY)!;

		expect(gateway.models).toEqual([
			{
				id: "acme/new-batch-model",
				label: "New Batch Model",
				mode: "segmented",
			},
			{
				id: "acme/new-live-model",
				label: "New Live Model",
				mode: "streaming",
			},
		]);
		expect(
			providers.find((provider) => provider.id === "mistral")?.models.length,
		).toBe(2);
		expect(
			providers.find((provider) => provider.id === "openai")?.models.map(
				(model) => model.id,
			),
		).toContain("gpt-transcribe");
	});

	test("ignores an empty discovered catalogue", () => {
		expect(
			listCapabilities({ env: {}, gatewayModels: [] }).find(
				(provider) => provider.id === GATEWAY,
			)?.models,
		).toHaveLength(16);
	});

	test("availability reflects the environment per provider", () => {
		const availableIds = (env: Record<string, string | undefined>) =>
			listCapabilities({ env })
				.filter((provider) => provider.available)
				.map((provider) => provider.id);

		expect(availableIds({})).toEqual([]);
		expect(availableIds({ MISTRAL_API_KEY: "env-key" })).toEqual(["mistral"]);
		expect(
			availableIds({
				AI_GATEWAY_API_KEY: "gateway-key",
				XAI_API_KEY: "xai-key",
			}),
		).toEqual([GATEWAY, "xai"]);
	});

	test("a supplied key marks only the matching provider available", () => {
		const providers = listCapabilities({
			env: {},
			apiKeyFor: (providerId) =>
				providerId === "openai" ? "sk-explicit" : undefined,
		});

		expect(
			providers.filter((provider) => provider.available).map((p) => p.id),
		).toEqual(["openai"]);
	});

	test("never exposes key values in the capability payload", () => {
		const payload = JSON.stringify(
			listCapabilities({
				env: { MISTRAL_API_KEY: "sk-env-secret" },
				apiKeyFor: () => "sk-explicit-secret",
			}),
		);

		expect(payload).not.toContain("sk-env-secret");
		expect(payload).not.toContain("sk-explicit-secret");
	});
});
