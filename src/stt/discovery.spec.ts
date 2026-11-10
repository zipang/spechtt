import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
	DISCOVERY_ENDPOINT,
	cachedGatewayCatalogue,
	discoverGatewayModels,
	parseLanguageModels,
	resetDiscoveryCache
} from "./discovery";

type FetchCall = {
	url: string;
	init: RequestInit | undefined;
};

function jsonResponse(body: unknown, status = 200): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "content-type": "application/json" }
	});
}

function fakeFetch(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
	const calls: FetchCall[] = [];
	const fetchImpl = (async (
		input: Parameters<typeof globalThis.fetch>[0],
		init?: RequestInit
	): Promise<Response> => {
		calls.push({ url: String(input), init });
		return handler(String(input), init);
	}) as typeof globalThis.fetch;
	return { calls, fetch: fetchImpl };
}

const transcription = (
	id: string,
	name: string,
	tags?: string[] | null
): Record<string, unknown> => ({
	id,
	name,
	type: "transcription",
	tags
});

describe("discoverGatewayModels", () => {
	beforeEach(() => {
		resetDiscoveryCache();
	});

	afterEach(() => {
		resetDiscoveryCache();
	});

	test("keeps transcription models and maps the streaming tag to the mode", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					transcription("openai/gpt-4o-transcribe", "GPT-4o Transcribe"),
					transcription("openai/gpt-transcribe", "GPT Transcribe", [
						"websocket-transcription"
					]),
					transcription("openai/whisper-1", "Whisper", null),
					transcription("openai/gpt-realtime-whisper", "GPT Realtime Whisper", [
						"websocket-realtime",
						"websocket-transcription"
					]),
					{
						id: "openai/tts-1",
						name: "TTS 1",
						type: "speech",
						tags: ["websocket-transcription"]
					}
				]
			})
		);

		const result = await discoverGatewayModels({ fetch, now: () => 1_000 });

		expect(result?.fetchedAt).toBe(1_000);
		const empty = { description: null, pricing: {} };
		expect(result?.models).toEqual([
			{
				id: "openai/gpt-realtime-whisper",
				label: "GPT Realtime Whisper",
				mode: "streaming",
				...empty
			},
			{
				id: "openai/gpt-transcribe",
				label: "GPT Transcribe",
				mode: "streaming",
				...empty
			},
			{
				id: "openai/gpt-4o-transcribe",
				label: "GPT-4o Transcribe",
				mode: "segmented",
				...empty
			},
			{ id: "openai/whisper-1", label: "Whisper", mode: "segmented", ...empty }
		]);
	});

	test("keeps the description and the raw pricing map", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					{
						id: "acme/stt",
						name: "Acme STT",
						type: "transcription",
						description: "  Fast, accurate, billed per second.  ",
						pricing: {
							transcription_duration_cost_per_second: "0.000030556",
							input: 42,
							nested: { nope: true }
						}
					},
					{
						id: "acme/no-meta",
						name: "No Meta",
						type: "transcription",
						description: "",
						pricing: null
					}
				]
			})
		);

		const result = await discoverGatewayModels({ fetch });

		expect(result?.models).toEqual([
			{
				id: "acme/stt",
				label: "Acme STT",
				mode: "segmented",
				description: "Fast, accurate, billed per second.",
				pricing: { transcription_duration_cost_per_second: "0.000030556" }
			},
			{
				id: "acme/no-meta",
				label: "No Meta",
				mode: "segmented",
				description: null,
				pricing: {}
			}
		]);
	});

	test("treats missing tags as batch and falls back to the id for an empty name", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					transcription("acme/empty", ""),
					{ id: "acme/missing-name", type: "transcription" },
					transcription("acme/empty-tags", "Empty Tags", [])
				]
			})
		);

		const result = await discoverGatewayModels({ fetch });

		const empty = { description: null, pricing: {} };
		expect(result?.models).toEqual([
			{
				id: "acme/empty-tags",
				label: "Empty Tags",
				mode: "segmented",
				...empty
			},
			{ id: "acme/empty", label: "acme/empty", mode: "segmented", ...empty },
			{
				id: "acme/missing-name",
				label: "acme/missing-name",
				mode: "segmented",
				...empty
			}
		]);
	});

	test("sorts by label, then id, deterministically across locales", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					transcription("acme/b", "Same"),
					transcription("acme/a", "Same"),
					transcription("acme/z", "Alpha"),
					transcription("acme/c", "Beta")
				]
			})
		);

		const result = await discoverGatewayModels({ fetch });

		expect(result?.models.map((model) => model.id)).toEqual([
			"acme/z",
			"acme/c",
			"acme/a",
			"acme/b"
		]);
	});

	test("performs an anonymous GET on the documented endpoint", async () => {
		const { calls, fetch } = fakeFetch(() =>
			jsonResponse({ data: [transcription("acme/stt", "STT")] })
		);

		await discoverGatewayModels({ fetch });

		expect(calls).toHaveLength(1);
		expect(calls[0]?.url).toBe(DISCOVERY_ENDPOINT);
		expect(calls[0]?.url).toBe("https://ai-gateway.vercel.sh/v1/models");
		expect(calls[0]?.init?.method ?? "GET").toBe("GET");

		const headers = new Headers(calls[0]?.init?.headers);
		expect(headers.has("authorization")).toBe(false);
		expect(headers.has("x-api-key")).toBe(false);
		expect(JSON.stringify(calls[0])).not.toContain("sk-");
	});

	test("honours an explicit endpoint for diagnostics and tests", async () => {
		const { calls, fetch } = fakeFetch(() =>
			jsonResponse({ data: [transcription("acme/stt", "STT")] })
		);

		await discoverGatewayModels({
			fetch,
			endpoint: "https://example.test/v1/models"
		});

		expect(calls[0]?.url).toBe("https://example.test/v1/models");
	});

	test("returns undefined on a non-2xx response", async () => {
		for (const status of [301, 400, 401, 429, 500, 503]) {
			resetDiscoveryCache();
			const { fetch } = fakeFetch(() =>
				jsonResponse({ data: [transcription("acme/stt", "STT")] }, status)
			);

			await expect(discoverGatewayModels({ fetch })).resolves.toBeUndefined();
		}
	});

	test("returns undefined when the request rejects", async () => {
		const { fetch } = fakeFetch(() => {
			throw new TypeError("network down");
		});

		await expect(discoverGatewayModels({ fetch })).resolves.toBeUndefined();
	});

	test("returns undefined on abort or timeout and clears the timer", async () => {
		const { fetch } = fakeFetch(
			(_url, init) =>
				new Promise<Response>((_resolve, reject) => {
					init?.signal?.addEventListener("abort", () => {
						reject(new DOMException("aborted", "AbortError"));
					});
				})
		);

		await expect(discoverGatewayModels({ fetch, timeoutMs: 5 })).resolves.toBeUndefined();
	});

	test("returns undefined on a non-JSON body", async () => {
		const { fetch } = fakeFetch(
			() =>
				new Response("<html>gateway error</html>", {
					status: 200,
					headers: { "content-type": "text/html" }
				})
		);

		await expect(discoverGatewayModels({ fetch })).resolves.toBeUndefined();
	});

	test("returns undefined on a malformed payload", async () => {
		const payloads = [
			{},
			{ data: null },
			{ data: {} },
			{ data: "nope" },
			{ models: [] },
			[],
			null
		];

		for (const payload of payloads) {
			resetDiscoveryCache();
			const { fetch } = fakeFetch(() => jsonResponse(payload));

			await expect(discoverGatewayModels({ fetch })).resolves.toBeUndefined();
		}
	});

	test("returns undefined when no transcription models are present", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					{ id: "openai/tts-1", name: "TTS", type: "speech" },
					{ id: "openai/embed", name: "Embed", type: "embedding" }
				]
			})
		);

		await expect(discoverGatewayModels({ fetch })).resolves.toBeUndefined();
	});

	test("caches the last success for 60 seconds of injected time", async () => {
		let now = 1_000;
		const { calls, fetch } = fakeFetch(() =>
			jsonResponse({ data: [transcription("acme/stt", "STT")] })
		);
		const options = { fetch, now: () => now };

		const first = await discoverGatewayModels(options);
		const second = await discoverGatewayModels(options);

		expect(first).toEqual(second);
		expect(calls).toHaveLength(1);

		now = 1_000 + 59_999;
		await discoverGatewayModels(options);
		expect(calls).toHaveLength(1);

		now = 1_000 + 60_000;
		await discoverGatewayModels(options);
		expect(calls).toHaveLength(2);
	});

	test("does not cache a failed discovery", async () => {
		let attempts = 0;
		const { calls, fetch } = fakeFetch(() => {
			attempts += 1;
			if (attempts === 1) {
				throw new TypeError("network down");
			}
			return jsonResponse({ data: [transcription("acme/stt", "STT")] });
		});
		const options = { fetch, now: () => 1_000 };

		await expect(discoverGatewayModels(options)).resolves.toBeUndefined();
		const retried = await discoverGatewayModels(options);

		expect(retried?.models).toHaveLength(1);
		expect(calls).toHaveLength(2);
	});

	test("single-flights concurrent calls", async () => {
		let resolveFetch: ((response: Response) => void) | undefined;
		const { calls, fetch } = fakeFetch(
			() =>
				new Promise<Response>((resolve) => {
					resolveFetch = resolve;
				})
		);
		const options = { fetch, now: () => 1_000 };

		const first = discoverGatewayModels(options);
		const second = discoverGatewayModels(options);
		expect(calls).toHaveLength(1);

		resolveFetch?.(jsonResponse({ data: [transcription("acme/stt", "STT")] }));
		const [one, two] = await Promise.all([first, second]);

		expect(one).toEqual(two);
		expect(one?.models.map((model) => model.id)).toEqual(["acme/stt"]);
	});

	test("resetDiscoveryCache forces the next call to refetch", async () => {
		const { calls, fetch } = fakeFetch(() =>
			jsonResponse({ data: [transcription("acme/stt", "STT")] })
		);
		const options = { fetch, now: () => 1_000 };

		await discoverGatewayModels(options);
		resetDiscoveryCache();
		await discoverGatewayModels(options);

		expect(calls).toHaveLength(2);
	});
});

describe("cachedGatewayCatalogue", () => {
	beforeEach(() => {
		resetDiscoveryCache();
	});

	afterEach(() => {
		resetDiscoveryCache();
	});

	test("exposes the last success synchronously until the TTL expires", async () => {
		let now = 5_000;
		const { fetch } = fakeFetch(() =>
			jsonResponse({ data: [transcription("acme/stt", "STT")] })
		);

		expect(cachedGatewayCatalogue(() => now)).toBeUndefined();

		await discoverGatewayModels({ fetch, now: () => now });

		expect(cachedGatewayCatalogue(() => now)?.models.map((model) => model.id)).toEqual([
			"acme/stt"
		]);

		now = 5_000 + 60_000;
		expect(cachedGatewayCatalogue(() => now)).toBeUndefined();
	});
});

describe("language model discovery", () => {
	beforeEach(() => {
		resetDiscoveryCache();
	});

	afterEach(() => {
		resetDiscoveryCache();
	});

	const language = (id: string, name: string): Record<string, unknown> => ({
		id,
		name,
		type: "language"
	});

	test("collects language entries while transcription models keep their modes", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					language("openai/gpt-4.1-mini", "GPT-4.1 mini"),
					transcription("openai/gpt-transcribe", "GPT Transcribe", [
						"websocket-transcription"
					]),
					language("anthropic/claude-sonnet-5.5", "Claude Sonnet 5.5"),
					{ id: "openai/tts-1", name: "TTS 1", type: "speech" }
				]
			})
		);

		const result = await discoverGatewayModels({ fetch });

		expect(result?.models.map((model) => model.id)).toEqual(["openai/gpt-transcribe"]);
		expect(result?.models[0]?.mode).toBe("streaming");
		expect(result?.languageModels).toEqual([
			{ id: "anthropic/claude-sonnet-5.5", label: "Claude Sonnet 5.5" },
			{ id: "openai/gpt-4.1-mini", label: "GPT-4.1 mini" }
		]);
	});

	test("reports an empty language list when the catalogue has none", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({ data: [transcription("acme/stt", "STT")] })
		);

		const result = await discoverGatewayModels({ fetch });

		expect(result?.languageModels).toEqual([]);
	});

	test("parses language entries directly, sorted by label and deduped", () => {
		const models = parseLanguageModels({
			data: [
				language("acme/b", "Same"),
				language("acme/a", "Same"),
				language("acme/z", "Alpha"),
				language("acme/alpha", "Alpha"),
				language("acme/empty", ""),
				{ id: "acme/no-name", type: "language" },
				{ id: "", name: "Empty Id", type: "language" },
				{ id: 42, name: "Numeric Id", type: "language" },
				transcription("openai/gpt-transcribe", "GPT Transcribe")
			]
		});

		expect(models).toEqual([
			{ id: "acme/alpha", label: "Alpha" },
			{ id: "acme/z", label: "Alpha" },
			{ id: "acme/a", label: "Same" },
			{ id: "acme/b", label: "Same" },
			{ id: "acme/empty", label: "acme/empty" },
			{ id: "acme/no-name", label: "acme/no-name" }
		]);
	});

	test("returns an empty list for a malformed payload", () => {
		for (const payload of [null, [], {}, { data: null }, { data: "nope" }]) {
			expect(parseLanguageModels(payload)).toEqual([]);
		}
	});

	test("exposes the cached language models with the cached catalogue", async () => {
		const { fetch } = fakeFetch(() =>
			jsonResponse({
				data: [
					transcription("acme/stt", "STT"),
					language("openai/gpt-4.1-mini", "GPT-4.1 mini")
				]
			})
		);

		await discoverGatewayModels({ fetch, now: () => 1_000 });

		expect(cachedGatewayCatalogue(() => 1_000)?.languageModels).toEqual([
			{ id: "openai/gpt-4.1-mini", label: "GPT-4.1 mini" }
		]);
	});
});
