/**
 * Dynamic transcription-model discovery through the Vercel AI Gateway.
 *
 * `GET /v1/models` is anonymous and returns every model with a `type` and
 * `tags`. Streaming transcription models carry `websocket-transcription`; the
 * AI SDK's `getAvailableModels()` exposes `modelType` but no tags, so it cannot
 * distinguish streaming from batch. This module is pure apart from the injected
 * `fetch` and clock, which keeps the tests hermetic.
 */

import type { TranscriptionMode } from "./registry";

/** Documented anonymous models endpoint. */
export const DISCOVERY_ENDPOINT = "https://ai-gateway.vercel.sh/v1/models";
/** A successful discovery is reused for this long. */
export const DISCOVERY_TTL_MS = 60_000;
/** Whole-request budget: a slow gateway must never stall the handshake. */
const DEFAULT_TIMEOUT_MS = 2000;

export interface DiscoveredModel {
	/** Gateway-qualified id (`provider/model`), passed through unchanged. */
	readonly id: string;
	/** Display name from the endpoint, or the id when the name is empty. */
	readonly label: string;
	readonly mode: TranscriptionMode;
	/** Gateway description, when it publishes one. */
	readonly description: string | null;
	/** Raw pricing map, values kept as strings so the UI controls formatting. */
	readonly pricing: Readonly<Record<string, string>>;
}

export interface DiscoveryResult {
	readonly models: readonly DiscoveredModel[];
	/** `now()` of the successful response, used for the TTL cache. */
	readonly fetchedAt: number;
}

export interface DiscoveryOptions {
	/** Injected for tests; defaults to `globalThis.fetch`. */
	readonly fetch?: typeof globalThis.fetch;
	readonly timeoutMs?: number;
	readonly endpoint?: string;
	/** Injected clock for the TTL cache; defaults to `Date.now`. */
	readonly now?: () => number;
}

let cache: DiscoveryResult | undefined;
let inFlight: Promise<DiscoveryResult | undefined> | undefined;

/** Clears the module cache; tests and diagnostics only. */
export function resetDiscoveryCache(): void {
	cache = undefined;
	inFlight = undefined;
}

function isFresh(result: DiscoveryResult, now: number): boolean {
	return now - result.fetchedAt < DISCOVERY_TTL_MS;
}

/**
 * The last successful discovery while it is still fresh, synchronously.
 *
 * The worker uses this to answer the first capabilities event with
 * `source: "discovered"` instead of `"builtin"`.
 */
export function cachedGatewayModels(
	now: () => number = Date.now,
): readonly DiscoveredModel[] | undefined {
	if (cache === undefined || !isFresh(cache, now())) {
		return undefined;
	}
	return cache.models;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

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

function parsePricing(value: unknown): Readonly<Record<string, string>> {
	if (!isRecord(value)) {
		return {};
	}
	const pricing: Record<string, string> = {};
	for (const [key, raw] of Object.entries(value)) {
		if (typeof raw === "string") {
			pricing[key] = raw;
		}
	}
	return pricing;
}

function parseModels(payload: unknown): readonly DiscoveredModel[] | undefined {
	if (!isRecord(payload) || !Array.isArray(payload.data)) {
		return undefined;
	}

	const models: DiscoveredModel[] = [];
	for (const entry of payload.data) {
		if (!isRecord(entry) || entry.type !== "transcription") {
			continue;
		}
		const id = entry.id;
		if (typeof id !== "string" || id.length === 0) {
			continue;
		}
		const name = typeof entry.name === "string" ? entry.name.trim() : "";
		const description =
			typeof entry.description === "string" && entry.description.trim().length > 0
				? entry.description.trim()
				: null;
		const tags = Array.isArray(entry.tags) ? entry.tags : [];
		const streaming = tags.includes("websocket-transcription");
		models.push({
			id,
			label: name.length > 0 ? name : id,
			mode: streaming ? "streaming" : "segmented",
			description,
			pricing: parsePricing(entry.pricing),
		});
	}

	if (models.length === 0) {
		return undefined;
	}
	return models.sort(compareByLabel);
}

async function requestModels(
	fetchImpl: typeof globalThis.fetch,
	endpoint: string,
	timeoutMs: number,
	now: () => number,
): Promise<DiscoveryResult | undefined> {
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), timeoutMs);
	try {
		const response = await fetchImpl(endpoint, {
			method: "GET",
			headers: { accept: "application/json" },
			signal: controller.signal,
		});
		if (!response.ok) {
			return undefined;
		}

		const payload: unknown = await response.json().catch(() => undefined);
		const models = parseModels(payload);
		if (models === undefined) {
			return undefined;
		}
		return { models, fetchedAt: now() };
	} catch {
		return undefined;
	} finally {
		clearTimeout(timer);
	}
}

/**
 * Fetches the gateway transcription catalogue, or `undefined` on any failure.
 *
 * Failures are never cached, and concurrent callers share a single request.
 */
export async function discoverGatewayModels(
	options: DiscoveryOptions = {},
): Promise<DiscoveryResult | undefined> {
	const now = options.now ?? Date.now;
	if (cache !== undefined && isFresh(cache, now())) {
		return cache;
	}
	if (inFlight !== undefined) {
		return inFlight;
	}

	const fetchImpl = options.fetch ?? globalThis.fetch;
	const endpoint = options.endpoint ?? DISCOVERY_ENDPOINT;
	const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;

	inFlight = requestModels(fetchImpl, endpoint, timeoutMs, now).then(
		(result) => {
			if (result !== undefined) {
				cache = result;
			}
			return result;
		},
	);

	try {
		return await inFlight;
	} finally {
		inFlight = undefined;
	}
}
