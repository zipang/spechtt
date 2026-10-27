import { describe, expect, test } from "bun:test";
import {
	MODEL_CAPABILITIES,
	PROVIDERS,
	getProvider,
	isProviderAvailable,
	listCapabilities,
	providerEnvVars,
	resolveModelCapability,
	resolveProviderApiKey,
} from "./registry";

describe("provider registry", () => {
	test("exposes the supported STT providers", () => {
		for (const provider of [
			"vercel",
			"gateway",
			"mistral",
			"openai",
			"xai",
			"elevenlabs",
		]) {
			expect(getProvider(provider)).toBeDefined();
		}

		expect(getProvider("unknown-provider")).toBeUndefined();
		expect(getProvider("")).toBeUndefined();
		expect(Object.keys(PROVIDERS).length).toBeGreaterThanOrEqual(6);
	});

	test("maps providers to their environment variables", () => {
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

	test("accepts both gateway environment variables", () => {
		expect(
			isProviderAvailable("vercel", undefined, {
				AI_GATEWAY_API_KEY: "gateway-key",
			}),
		).toBe(true);
		expect(
			isProviderAvailable("vercel", undefined, {
				VERCEL_AI_GATEWAY_API_KEY: "gateway-key",
			}),
		).toBe(true);
	});

	test("is unavailable without an explicit or environment key", () => {
		expect(isProviderAvailable("mistral", undefined, {})).toBe(false);
		expect(isProviderAvailable("openai", "", {})).toBe(false);
		expect(
			isProviderAvailable("vercel", undefined, {
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
			resolveProviderApiKey("vercel", undefined, {
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
		for (const [provider, model] of [
			["vercel", "openai/gpt-realtime-whisper"],
			["openai", "gpt-realtime-whisper"],
			["vercel", "xai/grok-stt"],
			["xai", "grok-stt"],
			["vercel", "elevenlabs/eleven-scribe-2-realtime"],
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
		for (const [provider, model] of [
			["vercel", "mistral/voxtral-mini-transcribe-3-0"],
			["mistral", "voxtral-mini-transcribe-3-0"],
			["vercel", "openai/whisper-1"],
			["openai", "whisper-1"],
			["openai", "gpt-4o-transcribe"],
		] as const) {
			expect(resolveModelCapability(provider, model)).toMatchObject({
				provider,
				model,
				mode: "segmented",
			});
		}
	});

	test("falls back to the provider's default mode for unknown models", () => {
		expect(resolveModelCapability("vercel", "acme/brand-new-live")).toMatchObject(
			{ mode: "streaming" },
		);
		expect(resolveModelCapability("mistral", "voxtral-custom")).toMatchObject(
			{ mode: "segmented" },
		);
		expect(resolveModelCapability("openai", "custom-transcribe")).toMatchObject(
			{ mode: "segmented" },
		);
	});

	test("returns undefined for unknown providers", () => {
		expect(resolveModelCapability("unknown", "some-model")).toBeUndefined();
	});
});

describe("capability listing", () => {
	test("lists every provider deterministically sorted by label", () => {
		const first = listCapabilities({ env: {} });
		const second = listCapabilities({ env: {} });

		expect(first).toEqual(second);
		expect(first.length).toBeGreaterThanOrEqual(5);
		expect(new Set(first.map((provider) => provider.id)).size).toBe(
			first.length,
		);

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

	test("every listed model has an explicit, valid transcription mode", () => {
		for (const provider of listCapabilities({ env: {} })) {
			for (const model of provider.models) {
				expect(["streaming", "segmented"]).toContain(model.mode);
				expect(MODEL_CAPABILITIES[model.id]).toBe(model.mode);
				expect(model.label.length).toBeGreaterThan(0);
			}
		}
	});

	test("lists gateway-qualified models on the gateway providers", () => {
		const providers = listCapabilities({ env: {} });
		const vercel = providers.find((provider) => provider.id === "vercel");
		expect(vercel).toBeDefined();
		expect(vercel?.models.map((model) => model.id)).toContain(
			"openai/gpt-realtime-whisper",
		);
		expect(vercel?.models.map((model) => model.id)).toContain(
			"openai/gpt-4o-transcribe",
		);

		const openai = providers.find((provider) => provider.id === "openai");
		expect(openai?.models.map((model) => model.id)).toContain(
			"gpt-realtime-whisper",
		);
		expect(openai?.models.map((model) => model.id)).not.toContain(
			"openai/gpt-realtime-whisper",
		);
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
		).toEqual(["vercel", "gateway", "xai"]);
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
