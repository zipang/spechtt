import {
	afterEach,
	beforeEach,
	describe,
	expect,
	spyOn,
	test,
} from "bun:test";
import { type OutboundEvent, serializeOutboundEvent } from "./protocol";
import type {
	EmitFn,
	SttAdapter,
	SttAdapterFactory,
	SttSession,
	SttSessionConfig,
} from "./adapters/types";
import { MistralAdapter } from "./adapters/mistral";
import { VercelAiSdkAdapter } from "./adapters/vercel-ai-sdk";
import type { DiscoveredModel, DiscoveryResult } from "./discovery";
import type { FfmpegTools } from "./file-input";
import {
	type DiscoverySource,
	type FileDeps,
	defaultAdapterFactory,
	runWorkerLoop,
	sanitizeDiagnostic,
} from "./worker";

const START = JSON.stringify({
	type: "start",
	sessionId: "session-1",
	provider: "vercel",
	model: "openai/gpt-realtime-whisper",
	sampleRate: 16000,
	apiKey: "sk-test",
});

const audio = (marker: string) =>
	JSON.stringify({
		type: "audio",
		pcm16Base64: Buffer.from(marker).toString("base64").padEnd(4, "="),
	});

async function* linesOf(items: string[]): AsyncGenerator<string> {
	for (const item of items) {
		yield item;
	}
}

class FakeSession implements SttSession {
	pushes: string[] = [];
	paused = false;
	aborted = false;
	stopped = false;

	constructor(
		readonly sessionId: string,
		private readonly emit: EmitFn,
	) {}

	pushAudio(pcm16Base64: string): void {
		if (this.paused) {
			return;
		}
		this.pushes.push(pcm16Base64);
		if (this.pushes.length === 1) {
			this.emit({ type: "partial", sessionId: this.sessionId, text: "hel" });
			this.emit({ type: "delta", sessionId: this.sessionId, delta: "lo" });
			this.emit({
				type: "committed",
				sessionId: this.sessionId,
				text: "hello",
			});
		}
	}

	pause(): void {
		this.paused = true;
	}

	resume(): void {
		this.paused = false;
	}

	async stop(): Promise<void> {
		this.stopped = true;
		this.emit({
			type: "stopped",
			sessionId: this.sessionId,
			fullText: "hello",
		});
	}

	async abort(): Promise<void> {
		this.aborted = true;
	}
}

class FakeAdapter implements SttAdapter {
	readonly mode = "streaming" as const;
	readonly sessions: FakeSession[] = [];

	constructor(private readonly events: OutboundEvent[]) {}

	start(config: SttSessionConfig, emit: EmitFn): SttSession {
		emit({ type: "started", sessionId: config.sessionId });
		const session = new FakeSession(config.sessionId, emit);
		this.sessions.push(session);
		return session;
	}
}

function fakeFactory() {
	const events: OutboundEvent[] = [];
	const adapter = new FakeAdapter(events);
	const factory: SttAdapterFactory = () => adapter;
	return { factory, adapter, events };
}

describe("runWorkerLoop", () => {
	const savedEnv: Record<string, string | undefined> = {};

	beforeEach(() => {
		for (const name of [
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY",
			"MISTRAL_API_KEY",
			"OPENAI_API_KEY",
			"XAI_API_KEY",
			"ELEVENLABS_API_KEY",
		]) {
			savedEnv[name] = process.env[name];
			delete process.env[name];
		}
	});

	afterEach(() => {
		for (const [name, value] of Object.entries(savedEnv)) {
			if (value === undefined) {
				delete process.env[name];
			} else {
				process.env[name] = value;
			}
		}
	});

	test("emits ready then the full lifecycle for start/audio/pause/stop", async () => {
		const { factory, adapter, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([
				START,
				audio("a1"),
				audio("a2"),
				audio("a3"),
				JSON.stringify({ type: "pause" }),
				audio("dropped"),
				JSON.stringify({ type: "resume" }),
				audio("a5"),
				JSON.stringify({ type: "stop" }),
			]),
			(event) => events.push(event),
			factory,
		);

		expect(events).toEqual([
			{ type: "ready" },
			{ type: "started", sessionId: "session-1" },
			{ type: "partial", sessionId: "session-1", text: "hel" },
			{ type: "delta", sessionId: "session-1", delta: "lo" },
			{ type: "committed", sessionId: "session-1", text: "hello" },
			{ type: "stopped", sessionId: "session-1", fullText: "hello" },
		]);

		const session = adapter.sessions[0]!;
		expect(session.pushes).toHaveLength(4);
		expect(session.stopped).toBe(true);
		expect(session.aborted).toBe(false);
	});

	test("forwards exactly four audio chunks around the pause gate", async () => {
		const { factory, adapter, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([
				START,
				audio("a1"),
				audio("a2"),
				audio("a3"),
				JSON.stringify({ type: "pause" }),
				audio("a4"),
				JSON.stringify({ type: "resume" }),
				audio("a5"),
				JSON.stringify({ type: "stop" }),
			]),
			(event) => events.push(event),
			factory,
		);

		const session = adapter.sessions[0]!;
		expect(session.pushes).toEqual([
			Buffer.from("a1").toString("base64").padEnd(4, "="),
			Buffer.from("a2").toString("base64").padEnd(4, "="),
			Buffer.from("a3").toString("base64").padEnd(4, "="),
			Buffer.from("a5").toString("base64").padEnd(4, "="),
		]);
	});

	test("keeps running after a missing API key", async () => {
		const { factory, adapter, events } = fakeFactory();
		const noKey = JSON.stringify({
			type: "start",
			sessionId: "no-key",
			provider: "vercel-ai-gateway",
			model: "openai/gpt-4o-transcribe",
			sampleRate: 16000,
		});

		await runWorkerLoop(
			linesOf([noKey, START, JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			factory,
		);

		expect(events[0]).toEqual({ type: "ready" });
		expect(events[1]).toMatchObject({
			type: "error",
			sessionId: "no-key",
			code: "missing_api_key",
		});
		expect(events[2]).toEqual({
			type: "started",
			sessionId: "session-1",
		});
		expect(events.at(-1)).toMatchObject({ type: "stopped" });
		expect(adapter.sessions).toHaveLength(1);
	});

	test("uses a provider environment variable when no explicit key is sent", async () => {
		process.env.AI_GATEWAY_API_KEY = "env-key";
		const configs: SttSessionConfig[] = [];
		const { factory } = fakeFactory();
		const capturingFactory: SttAdapterFactory = (config) => {
			configs.push(config);
			return factory(config);
		};

		await runWorkerLoop(
			linesOf([
				JSON.stringify({
					type: "start",
					sessionId: "env-session",
					provider: "vercel-ai-gateway",
					model: "openai/gpt-4o-transcribe",
					sampleRate: 16000,
				}),
				JSON.stringify({ type: "stop" }),
			]),
			() => {},
			capturingFactory,
		);

		expect(configs[0]?.apiKey).toBe("env-key");
	});

	test("rejects unknown providers without starting a session", async () => {
		const { factory, adapter, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([
				JSON.stringify({
					type: "start",
					sessionId: "bad",
					provider: "acme",
					model: "whatever",
					sampleRate: 16000,
					apiKey: "sk-test",
				}),
			]),
			(event) => events.push(event),
			factory,
		);

		expect(events).toEqual([
			{ type: "ready" },
			{
				type: "error",
				sessionId: "bad",
				code: "unsupported_provider",
				message: expect.stringContaining("acme"),
			},
		]);
		expect(adapter.sessions).toHaveLength(0);
	});

	test("reports malformed NDJSON without crashing or leaking the line", async () => {
		const { factory, events } = fakeFactory();
		const leakyLine = '{"type":"start","apiKey":"sk-super-secret"';

		await runWorkerLoop(
			linesOf([leakyLine, JSON.stringify({ type: "nope" })]),
			(event) => events.push(event),
			factory,
		);

		expect(events).toEqual([
			{ type: "ready" },
			{
				type: "error",
				code: "invalid_protocol",
				message: "Invalid JSON payload",
			},
			{
				type: "error",
				code: "invalid_protocol",
				message: "Unknown command type: nope",
			},
		]);
		expect(JSON.stringify(events)).not.toContain("sk-super-secret");
	});

	test("aborts the active session cleanly on EOF", async () => {
		const { factory, adapter, events } = fakeFactory();

		await expect(
			runWorkerLoop(
				linesOf([START, audio("a1")]),
				(event) => events.push(event),
				factory,
			),
		).resolves.toBeUndefined();

		expect(events).toEqual([
			{ type: "ready" },
			{ type: "started", sessionId: "session-1" },
			{ type: "partial", sessionId: "session-1", text: "hel" },
			{ type: "delta", sessionId: "session-1", delta: "lo" },
			{ type: "committed", sessionId: "session-1", text: "hello" },
		]);
		const session = adapter.sessions[0]!;
		expect(session.aborted).toBe(true);
		expect(session.stopped).toBe(false);
	});

	test("stops the previous session when a new start arrives", async () => {
		const { factory, adapter, events } = fakeFactory();
		const second = JSON.stringify({
			type: "start",
			sessionId: "session-2",
			provider: "vercel",
			model: "openai/gpt-realtime-whisper",
			sampleRate: 16000,
			apiKey: "sk-test",
		});

		await runWorkerLoop(
			linesOf([START, second, JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			factory,
		);

		expect(adapter.sessions).toHaveLength(2);
		expect(adapter.sessions[0]?.stopped).toBe(true);
		expect(events).toEqual([
			{ type: "ready" },
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "hello" },
			{ type: "started", sessionId: "session-2" },
			{ type: "stopped", sessionId: "session-2", fullText: "hello" },
		]);
	});

	test("reports adapter factory failures as stream_failed", async () => {
		const events: OutboundEvent[] = [];
		const factory: SttAdapterFactory = () => {
			throw new Error("factory exploded");
		};

		await runWorkerLoop(linesOf([START]), (event) => events.push(event), factory);

		expect(events).toEqual([
			{ type: "ready" },
			{
				type: "error",
				sessionId: "session-1",
				code: "stream_failed",
				message: "factory exploded",
			},
		]);
	});

	test("selects the adapter from the model capability mode", async () => {
		const streaming = await defaultAdapterFactory({
			sessionId: "s",
			provider: "vercel",
			model: "openai/gpt-realtime-whisper",
			sampleRate: 16000,
			apiKey: "sk-test",
		});
		const segmented = await defaultAdapterFactory({
			sessionId: "s",
			provider: "vercel-ai-gateway",
			model: "openai/gpt-4o-transcribe",
			sampleRate: 16000,
			apiKey: "sk-test",
		});

		expect(streaming).toBeInstanceOf(VercelAiSdkAdapter);
		expect(segmented).toBeInstanceOf(MistralAdapter);
	});
});

describe("runWorkerLoop capabilities handshake", () => {
	const capabilitiesLine = (overrides: Record<string, unknown> = {}) =>
		JSON.stringify({ type: "capabilities", ...overrides });

	const failingDiscovery: DiscoverySource = {
		cached: () => undefined,
		discover: async () => undefined,
	};

	function deferred<T>() {
		let resolve!: (value: T) => void;
		const promise = new Promise<T>((settle) => {
			resolve = settle;
		});
		return { promise, resolve };
	}

	const discoveredModels: DiscoveredModel[] = [
		{
			id: "acme/live",
			label: "Acme Live",
			mode: "streaming",
			description: "Acme live transcription.",
			pricing: { transcription_duration_cost_per_second: "0.00004" },
		},
		{
			id: "acme/batch",
			label: "Acme Batch",
			mode: "segmented",
			description: null,
			pricing: {},
		},
	];

	const discoveryResult: DiscoveryResult = {
		models: discoveredModels,
		fetchedAt: 1_000,
	};

	const savedEnv: Record<string, string | undefined> = {};

	beforeEach(() => {
		for (const name of [
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY",
			"MISTRAL_API_KEY",
			"OPENAI_API_KEY",
			"XAI_API_KEY",
			"ELEVENLABS_API_KEY",
		]) {
			savedEnv[name] = process.env[name];
			delete process.env[name];
		}
	});

	afterEach(() => {
		for (const [name, value] of Object.entries(savedEnv)) {
			if (value === undefined) {
				delete process.env[name];
			} else {
				process.env[name] = value;
			}
		}
	});

	const capabilitiesEvent = (
		events: OutboundEvent[],
	): Extract<OutboundEvent, { type: "capabilities" }> => {
		const event = events.find((candidate) => candidate.type === "capabilities");
		expect(event).toBeDefined();
		return event as Extract<OutboundEvent, { type: "capabilities" }>;
	};

	test("answers capabilities before any session exists", async () => {
		const { factory, adapter, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([capabilitiesLine({ requestId: "cap-1" })]),
			(event) => events.push(event),
			factory,
			failingDiscovery,
		);

		expect(events[0]).toEqual({ type: "ready" });
		const caps = capabilitiesEvent(events);
		expect(caps.requestId).toBe("cap-1");
		expect(caps.source).toBe("builtin");
		expect(caps.providers).toHaveLength(1);
		for (const provider of caps.providers) {
			expect(provider.available).toBe(false);
			expect(provider.models.length).toBeGreaterThan(0);
		}
		expect(adapter.sessions).toHaveLength(0);
	});

	test("omits requestId when the request had none", async () => {
		const { factory, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([capabilitiesLine()]),
			(event) => events.push(event),
			factory,
			failingDiscovery,
		);

		const caps = capabilitiesEvent(events);
		expect("requestId" in caps).toBe(false);
	});

	test("answers capabilities during an active session without disturbing it", async () => {
		const { factory, adapter, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([
				START,
				capabilitiesLine({ requestId: "cap-live" }),
				audio("a1"),
				JSON.stringify({ type: "stop" }),
			]),
			(event) => events.push(event),
			factory,
			failingDiscovery,
		);

		expect(events.map((event) => event.type)).toEqual([
			"ready",
			"started",
			"capabilities",
			"partial",
			"delta",
			"committed",
			"stopped",
		]);
		expect(capabilitiesEvent(events).requestId).toBe("cap-live");
		expect(adapter.sessions).toHaveLength(1);
		expect(adapter.sessions[0]?.stopped).toBe(true);
		expect(adapter.sessions[0]?.aborted).toBe(false);
	});

	test("rejects a bad capabilities request and keeps serving", async () => {
		const { factory, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([
				JSON.stringify({ type: "capabilities", apiKey: 42 }),
				capabilitiesLine({ requestId: "cap-2" }),
			]),
			(event) => events.push(event),
			factory,
			failingDiscovery,
		);

		expect(events[0]).toEqual({ type: "ready" });
		expect(events[1]).toEqual({
			type: "error",
			code: "invalid_protocol",
			message: expect.stringContaining("capabilities.apiKey"),
		});
		expect(capabilitiesEvent(events).requestId).toBe("cap-2");
	});

	test("never emits a supplied api key", async () => {
		const { factory, events } = fakeFactory();
		const secret = "sk-capability-secret";

		await runWorkerLoop(
			linesOf([capabilitiesLine({ requestId: "cap-3", apiKey: secret })]),
			(event) => events.push(event),
			factory,
			failingDiscovery,
		);

		const serialized = events.map(serializeOutboundEvent).join("");
		expect(serialized).not.toContain(secret);
		expect(JSON.stringify(events)).not.toContain(secret);
	});

	function capabilitiesEvents(
		events: OutboundEvent[],
	): Array<Extract<OutboundEvent, { type: "capabilities" }>> {
		return events.filter(
			(event): event is Extract<OutboundEvent, { type: "capabilities" }> =>
				event.type === "capabilities",
		);
	}

	const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

	test("answers immediately as builtin, then refreshes to discovered models", async () => {
		const { factory, events } = fakeFactory();
		const gate = deferred<DiscoveryResult | undefined>();
		const source: DiscoverySource = {
			cached: () => undefined,
			discover: () => gate.promise,
		};

		// The refresh is only delivered while the worker still consumes input
		// (a result that lands after EOF is dropped, see the dedicated test),
		// so hold `stdin` open until the discovery has been emitted.
		let releaseInput!: () => void;
		const inputOpen = new Promise<void>((resolve) => {
			releaseInput = resolve;
		});
		async function* openLines(): AsyncGenerator<string> {
			yield capabilitiesLine({ requestId: "cap-1" });
			await inputOpen;
		}

		const run = runWorkerLoop(
			openLines(),
			(event) => events.push(event),
			factory,
			source,
		);
		await tick();

		// The first event landed even though discovery has not resolved yet.
		expect(events.map((event) => event.type)).toEqual([
			"ready",
			"capabilities",
		]);
		const builtin = capabilitiesEvents(events)[0]!;
		expect(builtin.source).toBe("builtin");
		expect(builtin.requestId).toBe("cap-1");
		expect(
			builtin.providers
				.find((provider) => provider.id === "vercel-ai-gateway")
				?.models.map((model) => model.id),
		).toContain("openai/gpt-realtime-whisper");

		gate.resolve(discoveryResult);
		await tick();
		releaseInput();
		await run;

		const [first, second] = capabilitiesEvents(events);
		expect(second).toBeDefined();
		expect(first?.source).toBe("builtin");
		expect(second?.source).toBe("discovered");
		expect(second?.requestId).toBe("cap-1");

		const refreshed = second!.providers;
		expect(
			refreshed
				.find((provider) => provider.id === "vercel-ai-gateway")
				?.models.map((model) => model.id),
		).toEqual(["acme/batch", "acme/live"]);
		// Only the gateway provider is offered for now.
		expect(refreshed).toHaveLength(1);
	});

	test("answers once as discovered when the cache is fresh", async () => {
		const { factory, events } = fakeFactory();
		let discoverCalls = 0;
		const source: DiscoverySource = {
			cached: () => discoveredModels,
			discover: async () => {
				discoverCalls += 1;
				return discoveryResult;
			},
		};

		await runWorkerLoop(
			linesOf([capabilitiesLine({ requestId: "cap-cache" })]),
			(event) => events.push(event),
			factory,
			source,
		);

		const caps = capabilitiesEvents(events);
		expect(caps).toHaveLength(1);
		expect(caps[0]?.source).toBe("discovered");
		expect(caps[0]?.requestId).toBe("cap-cache");
		expect(discoverCalls).toBe(0);
	});

	test("keeps only the builtin event when discovery fails", async () => {
		const { factory, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([capabilitiesLine({ requestId: "cap-fail" })]),
			(event) => events.push(event),
			factory,
			failingDiscovery,
		);

		const caps = capabilitiesEvents(events);
		expect(caps).toHaveLength(1);
		expect(caps[0]?.source).toBe("builtin");
		expect(events.some((event) => event.type === "error")).toBe(false);
	});

	test("keeps only the builtin event when discovery rejects", async () => {
		const { factory, events } = fakeFactory();
		const source: DiscoverySource = {
			cached: () => undefined,
			discover: async () => {
				throw new Error("gateway down");
			},
		};

		await runWorkerLoop(
			linesOf([capabilitiesLine()]),
			(event) => events.push(event),
			factory,
			source,
		);

		expect(capabilitiesEvents(events)).toHaveLength(1);
		expect(events.some((event) => event.type === "error")).toBe(false);
	});

	test("drops a discovery result that resolves after stdin EOF", async () => {
		const { factory, events } = fakeFactory();
		const gate = deferred<DiscoveryResult | undefined>();
		const source: DiscoverySource = {
			cached: () => undefined,
			discover: () => gate.promise,
		};

		const run = runWorkerLoop(
			linesOf([capabilitiesLine()]),
			(event) => events.push(event),
			factory,
			source,
		);
		await tick();

		gate.resolve(discoveryResult);
		await expect(run).resolves.toBeUndefined();

		expect(capabilitiesEvents(events)).toHaveLength(1);
		expect(capabilitiesEvents(events)[0]?.source).toBe("builtin");
	});

	test("a discovered refresh never disturbs an active session", async () => {
		const { factory, adapter, events } = fakeFactory();

		await runWorkerLoop(
			linesOf([
				START,
				capabilitiesLine({ requestId: "cap-live" }),
				audio("a1"),
				JSON.stringify({ type: "stop" }),
			]),
			(event) => events.push(event),
			factory,
			{ cached: () => undefined, discover: async () => discoveryResult },
		);

		expect(capabilitiesEvents(events)).toHaveLength(2);
		expect(events.map((event) => event.type)).toContain("stopped");
		expect(adapter.sessions).toHaveLength(1);
		expect(adapter.sessions[0]?.stopped).toBe(true);
		expect(adapter.sessions[0]?.aborted).toBe(false);
	});

	test("never passes the API key to discovery", async () => {
		const { factory, events } = fakeFactory();
		const secret = "sk-discovery-secret";
		const argumentCounts: number[] = [];
		const source: DiscoverySource = {
			cached: () => undefined,
			discover: (async (...args: unknown[]) => {
				argumentCounts.push(args.length);
				return discoveryResult;
			}) as DiscoverySource["discover"],
		};

		await runWorkerLoop(
			linesOf([capabilitiesLine({ apiKey: secret })]),
			(event) => events.push(event),
			factory,
			source,
		);

		expect(argumentCounts).toEqual([0]);
		expect(JSON.stringify(events)).not.toContain(secret);
	});
});

const FILE_COMMAND = JSON.stringify({
	type: "transcribeFile",
	sessionId: "file-1",
	provider: "vercel-ai-gateway",
	model: "openai/gpt-4o-transcribe",
	path: "/home/user/clips/interview.mp4",
	apiKey: "sk-file",
});

class FakeFileSession implements SttSession {
	stopped = false;
	aborted = false;
	pushes: string[] = [];

	constructor(
		readonly sessionId: string,
		private readonly emit: EmitFn,
	) {}

	pushAudio(pcm16Base64: string): void {
		this.pushes.push(pcm16Base64);
	}

	pause(): void {}

	resume(): void {}

	async stop(): Promise<void> {
		this.stopped = true;
		this.emit({
			type: "stopped",
			sessionId: this.sessionId,
			fullText: "file text",
		});
	}

	async abort(): Promise<void> {
		this.aborted = true;
	}
}

function fakeFileDeps(options: { ffmpeg?: FfmpegTools } = {}) {
	const configs: (SttSessionConfig & { path: string })[] = [];
	const sessions: FakeFileSession[] = [];
	let resolutions = 0;
	const deps: FileDeps = {
		findFfmpeg: () => {
			resolutions += 1;
			return options.ffmpeg ?? { ffmpeg: "/usr/bin/ffmpeg", ffprobe: "/usr/bin/ffprobe" };
		},
		createSession: (config, emit) => {
			configs.push(config);
			const session = new FakeFileSession(config.sessionId, emit);
			sessions.push(session);
			return session;
		},
	};

	return { deps, configs, sessions, resolutions: () => resolutions };
}

describe("runWorkerLoop transcribeFile", () => {
	const savedEnv: Record<string, string | undefined> = {};

	beforeEach(() => {
		for (const name of [
			"AI_GATEWAY_API_KEY",
			"VERCEL_AI_GATEWAY_API_KEY",
			"MISTRAL_API_KEY",
			"OPENAI_API_KEY",
			"XAI_API_KEY",
			"ELEVENLABS_API_KEY",
		]) {
			savedEnv[name] = process.env[name];
			delete process.env[name];
		}
	});

	afterEach(() => {
		for (const [name, value] of Object.entries(savedEnv)) {
			if (value === undefined) {
				delete process.env[name];
			} else {
				process.env[name] = value;
			}
		}
	});

	test("routes the path to a file session instead of the live adapter", async () => {
		const { factory, adapter, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([FILE_COMMAND, JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(adapter.sessions).toHaveLength(0);
		expect(files.configs).toEqual([
			{
				sessionId: "file-1",
				provider: "vercel-ai-gateway",
				model: "openai/gpt-4o-transcribe",
				sampleRate: 16_000,
				apiKey: "sk-file",
				path: "/home/user/clips/interview.mp4",
			},
		]);
		expect(events).toEqual([
			{ type: "ready" },
			{ type: "stopped", sessionId: "file-1", fullText: "file text" },
		]);
		expect(files.sessions[0]?.stopped).toBe(true);
	});

	test("never forwards audio chunks to a file session", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([FILE_COMMAND, audio("a1"), JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(files.sessions[0]?.pushes).toEqual([]);
	});

	test("uses the provider environment variable when no key is sent", async () => {
		process.env.AI_GATEWAY_API_KEY = "env-key";
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([
				JSON.stringify({ ...JSON.parse(FILE_COMMAND), apiKey: undefined }),
				JSON.stringify({ type: "stop" }),
			]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(files.configs[0]?.apiKey).toBe("env-key");
	});

	test("rejects an unknown provider without creating a session", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([
				JSON.stringify({ ...JSON.parse(FILE_COMMAND), provider: "acme" }),
			]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(events[1]).toMatchObject({
			type: "error",
			sessionId: "file-1",
			code: "unsupported_provider",
		});
		expect(files.configs).toHaveLength(0);
		expect(files.resolutions()).toBe(0);
	});

	test("refuses a streaming model without creating a session or touching ffmpeg", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([
				JSON.stringify({
					...JSON.parse(FILE_COMMAND),
					model: "openai/gpt-realtime-whisper",
				}),
			]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		const types = events.map((event) => event.type);
		expect(types).toEqual(["ready", "error"]);
		expect(types).not.toContain("started");
		expect(types).not.toContain("stopped");
		expect(events[1]).toMatchObject({
			type: "error",
			sessionId: "file-1",
			code: "unsupported_file_model",
		});
		expect((events[1] as { message: string }).message).toContain(
			"openai/gpt-realtime-whisper",
		);
		expect(files.configs).toHaveLength(0);
		expect(files.resolutions()).toBe(0);
	});

	test("reports a missing key and names the provider environment variables", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([JSON.stringify({ ...JSON.parse(FILE_COMMAND), apiKey: undefined })]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(events[1]).toMatchObject({
			type: "error",
			sessionId: "file-1",
			code: "missing_api_key",
		});
		expect((events[1] as { message: string }).message).toContain(
			"AI_GATEWAY_API_KEY",
		);
		expect(files.configs).toHaveLength(0);
	});

	test("emits ffmpeg_missing without starting a session or a started event", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();
		files.deps.findFfmpeg = () => undefined;

		await runWorkerLoop(
			linesOf([FILE_COMMAND]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		const types = events.map((event) => event.type);
		expect(types).toEqual(["ready", "error"]);
		expect(types).not.toContain("started");
		expect(events[1]).toMatchObject({
			type: "error",
			sessionId: "file-1",
			code: "ffmpeg_missing",
		});
		expect((events[1] as { message: string }).message).toContain("ffmpeg");
		expect(files.configs).toHaveLength(0);
	});

	test("stops the previous live session before starting the file session", async () => {
		const { factory, adapter, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([START, FILE_COMMAND, JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(adapter.sessions[0]?.stopped).toBe(true);
		expect(files.sessions).toHaveLength(1);
		expect(events.map((event) => event.type)).toEqual([
			"ready",
			"started",
			"stopped",
			"stopped",
		]);
	});

	test("stops the active file session when a new start arrives", async () => {
		const { factory, adapter, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([FILE_COMMAND, START, JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(files.sessions[0]?.stopped).toBe(true);
		expect(adapter.sessions).toHaveLength(1);
		expect(adapter.sessions[0]?.stopped).toBe(true);
	});

	test("aborts an active file session on stdin EOF", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();

		await runWorkerLoop(
			linesOf([FILE_COMMAND]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(files.sessions[0]?.aborted).toBe(true);
	});

	test("degrades a throwing file session factory to stream_failed", async () => {
		const { factory, events } = fakeFactory();
		const files = fakeFileDeps();
		files.deps.createSession = () => {
			throw new Error("session blew up");
		};

		await runWorkerLoop(
			linesOf([FILE_COMMAND]),
			(event) => events.push(event),
			factory,
			undefined,
			files.deps,
		);

		expect(events[1]).toEqual({
			type: "error",
			sessionId: "file-1",
			code: "stream_failed",
			message: "session blew up",
		});
	});
});

describe("worker diagnostics", () => {
	let errorSpy: ReturnType<typeof spyOn>;

	beforeEach(() => {
		errorSpy = spyOn(console, "error").mockImplementation(() => {});
	});

	afterEach(() => {
		errorSpy.mockRestore();
	});

	test("a clean empty stop emits no error event and no diagnostic line", async () => {
		const events: OutboundEvent[] = [];
		const emptyFactory: SttAdapterFactory = (config) => ({
			mode: "streaming",
			start: (sessionConfig: SttSessionConfig, emit: EmitFn): SttSession => {
				emit({ type: "started", sessionId: sessionConfig.sessionId });
				return {
					sessionId: sessionConfig.sessionId,
					pushAudio: () => {},
					pause: () => {},
					resume: () => {},
					stop: async () => {
						emit({
							type: "stopped",
							sessionId: sessionConfig.sessionId,
							fullText: "",
						});
					},
					abort: async () => {},
				};
			},
		});

		await runWorkerLoop(
			linesOf([START, JSON.stringify({ type: "stop" })]),
			(event) => events.push(event),
			emptyFactory,
		);

		expect(events).toEqual([
			{ type: "ready" },
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
		expect(events.some((event) => event.type === "error")).toBe(false);
		expect(errorSpy).not.toHaveBeenCalled();
	});

	test("the reported repro stays silent end to end through the real adapter", async () => {
		const events: OutboundEvent[] = [];
		const silentAudio = JSON.stringify({
			type: "audio",
			pcm16Base64: Buffer.alloc(640).toString("base64"),
		});
		// A provider stream that rejects an empty/voiceless take, the way the
		// reporting provider did on stop.
		const rejectingAdapter = new VercelAiSdkAdapter({
			resolveModel: () => "test-model",
			streamTranscribe: (options) => {
				// Drain the audio first, then fail on the flush — exactly what
				// the reporting provider did when the take was voiceless.
				const fullStream = (async function* () {
					for await (const _chunk of options.audio) {
						// Keep reading until the session closes the stream.
					}
					throw new Error("Nothing was recorded.");
				})();
				return {
					fullStream,
					text: Promise.resolve(""),
					segments: Promise.resolve([]),
					language: Promise.resolve(undefined),
					durationInSeconds: Promise.resolve(undefined),
					warnings: Promise.resolve([]),
					responses: Promise.resolve([]),
					providerMetadata: Promise.resolve({}),
				} as never;
			},
		});

		await runWorkerLoop(
			linesOf([
				START,
				silentAudio,
				silentAudio,
				JSON.stringify({ type: "stop" }),
			]),
			(event) => events.push(event),
			() => rejectingAdapter,
		);

		expect(events).toEqual([
			{ type: "ready" },
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
		expect(errorSpy).not.toHaveBeenCalled();
	});

	test("writes one redacted diagnostic per error without changing stdout", async () => {
		const { factory, events } = fakeFactory();
		const unknownProvider = JSON.stringify({
			type: "start",
			sessionId: "bad-provider",
			provider: "acme",
			model: "whatever",
			sampleRate: 16000,
			apiKey: "sk-super-secret",
		});

		await runWorkerLoop(
			linesOf([unknownProvider]),
			(event) => events.push(event),
			factory,
		);

		expect(events).toEqual([
			{ type: "ready" },
			{
				type: "error",
				sessionId: "bad-provider",
				code: "unsupported_provider",
				message: expect.stringContaining("acme"),
			},
		]);
		const lines = errorSpy.mock.calls.map((call: unknown[]) => String(call[0]));
		expect(lines).toHaveLength(1);
		expect(lines[0]).toContain("code=unsupported_provider");
		expect(lines[0]).toContain("sessionId=bad-provider");
		expect(lines[0]).not.toContain("sk-super-secret");
	});

	test("sanitizeDiagnostic collapses and redacts credential material", () => {
		const raw =
			"Authorization: Bearer sk-live-secret\napi_key=sk-other timeout\tkey=plain";
		const sanitized = sanitizeDiagnostic(raw);

		expect(sanitized).not.toContain("sk-live-secret");
		expect(sanitized).not.toContain("sk-other");
		expect(sanitized).not.toContain("plain");
		expect(sanitized).not.toContain("Authorization");
		expect(sanitized).not.toContain("\n");
		expect(sanitized).toContain("[REDACTED]");
		expect(sanitized.length).toBeLessThanOrEqual(120);
	});
});
