import { describe, expect, test } from "bun:test";
import {
	type CapabilitiesCommand,
	type InboundCommand,
	type OutboundEvent,
	parseInboundLine,
	redactCommand,
	serializeOutboundEvent,
} from "./protocol";

const startLine = (overrides: Record<string, unknown> = {}) =>
	JSON.stringify({
		type: "start",
		sessionId: "session-1",
		provider: "mistral",
		model: "voxtral-mini-transcribe-3-0",
		sampleRate: 16000,
		...overrides,
	});

describe("parseInboundLine", () => {
	test("parses a start command with an API key", () => {
		const result = parseInboundLine(
			startLine({ apiKey: "sk-secret" }),
		);

		expect(result).toEqual({
			ok: true,
			command: {
				type: "start",
				sessionId: "session-1",
				provider: "mistral",
				model: "voxtral-mini-transcribe-3-0",
				sampleRate: 16000,
				apiKey: "sk-secret",
			},
		});
	});

	test("parses a start command without an API key", () => {
		const result = parseInboundLine(startLine());

		expect(result.ok).toBe(true);
		if (!result.ok) return;
		expect(result.command).toEqual({
			type: "start",
			sessionId: "session-1",
			provider: "mistral",
			model: "voxtral-mini-transcribe-3-0",
			sampleRate: 16000,
		});
		expect("apiKey" in result.command).toBe(false);
	});

	test("parses audio, pause, resume and stop commands", () => {
		const audio = parseInboundLine(
			JSON.stringify({ type: "audio", pcm16Base64: "AAAA" }),
		);
		expect(audio).toEqual({
			ok: true,
			command: { type: "audio", pcm16Base64: "AAAA" },
		});

		for (const type of ["pause", "resume", "stop"] as const) {
			expect(parseInboundLine(JSON.stringify({ type }))).toEqual({
				ok: true,
				command: { type },
			});
		}
	});

	test("accepts an empty audio payload as zero bytes", () => {
		expect(
			parseInboundLine(
				JSON.stringify({ type: "audio", pcm16Base64: "" }),
			),
		).toEqual({
			ok: true,
			command: { type: "audio", pcm16Base64: "" },
		});
	});

	test("treats unexpected extra fields as non-fatal", () => {
		const result = parseInboundLine(
			JSON.stringify({ type: "stop", extra: true }),
		);

		expect(result).toEqual({ ok: true, command: { type: "stop" } });
	});

	test("rejects malformed JSON with invalid_protocol", () => {
		const result = parseInboundLine("{ not json");

		expect(result.ok).toBe(false);
		if (result.ok) return;
		expect(result.code).toBe("invalid_protocol");
		expect(result.message).toContain("Invalid JSON");
	});

	test("rejects blank lines with invalid_protocol", () => {
		const result = parseInboundLine("   ");

		expect(result.ok).toBe(false);
		if (result.ok) return;
		expect(result.code).toBe("invalid_protocol");
	});

	test("rejects unknown command types", () => {
		const result = parseInboundLine(
			JSON.stringify({ type: "flush" }),
		);

		expect(result.ok).toBe(false);
		if (result.ok) return;
		expect(result.code).toBe("invalid_protocol");
		expect(result.message).toContain("Unknown command type");
	});

	test("rejects non-object payloads", () => {
		for (const line of ["null", "42", '"start"', "[1,2,3]"]) {
			const result = parseInboundLine(line);
			expect(result.ok).toBe(false);
			if (result.ok) continue;
			expect(result.code).toBe("invalid_protocol");
		}
	});

	test("rejects start commands with missing or invalid fields", () => {
		const invalid: Array<Record<string, unknown>> = [
			{ sessionId: undefined },
			{ sessionId: "" },
			{ sessionId: 7 },
			{ provider: undefined },
			{ provider: "" },
			{ model: undefined },
			{ model: 12 },
			{ sampleRate: undefined },
			{ sampleRate: 0 },
			{ sampleRate: -16000 },
			{ sampleRate: 16000.5 },
			{ sampleRate: "16000" },
			{ apiKey: 42 },
		];

		for (const overrides of invalid) {
			const result = parseInboundLine(startLine(overrides));
			expect(result.ok).toBe(false);
			if (result.ok) continue;
			expect(result.code).toBe("invalid_protocol");
		}
	});

	test("rejects audio commands with missing or invalid payloads", () => {
		const invalid = [
			{ type: "audio" },
			{ type: "audio", pcm16Base64: 42 },
			{ type: "audio", pcm16Base64: "not base64!" },
			{ type: "audio", pcm16Base64: "AAA" },
		];

		for (const payload of invalid) {
			const result = parseInboundLine(JSON.stringify(payload));
			expect(result.ok).toBe(false);
			if (result.ok) continue;
			expect(result.code).toBe("invalid_protocol");
		}
	});
});

describe("serializeOutboundEvent", () => {
	test("serializes NDJSON with a trailing newline", () => {
		const event: OutboundEvent = {
			type: "partial",
			sessionId: "session-1",
			text: "hello",
		};

		const line = serializeOutboundEvent(event);

		expect(line.endsWith("\n")).toBe(true);
		expect(line.split("\n").filter(Boolean)).toHaveLength(1);
		expect(JSON.parse(line)).toEqual(event);
	});

	test("serializes every outbound event variant", () => {
		const events: OutboundEvent[] = [
			{ type: "ready" },
			{ type: "started", sessionId: "s" },
			{ type: "partial", sessionId: "s", text: "t" },
			{ type: "delta", sessionId: "s", delta: "d" },
			{ type: "committed", sessionId: "s", text: "c" },
			{ type: "stopped", sessionId: "s", fullText: "f" },
			{
				type: "error",
				sessionId: "s",
				code: "stream_failed",
				message: "boom",
			},
			{
				type: "error",
				code: "invalid_protocol",
				message: "bad line",
			},
			{
				type: "capabilities",
				requestId: "cap-1",
				source: "builtin",
				providers: [
					{
						id: "vercel-ai-gateway",
						label: "Vercel AI Gateway",
						available: false,
						keyEnvVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
						models: [
							{
								id: "openai/gpt-realtime-whisper",
								label: "gpt-realtime-whisper",
								mode: "streaming",
								description: null,
								pricing: {},
							},
						],
					},
				],
			},
			{
				type: "capabilities",
				source: "discovered",
				providers: [],
			},
		];

		for (const event of events) {
			expect(JSON.parse(serializeOutboundEvent(event))).toEqual(event);
		}
	});
});

describe("redactCommand", () => {
	test("masks the start API key without mutating the input", () => {
		const command: InboundCommand = {
			type: "start",
			sessionId: "session-1",
			provider: "vercel",
			model: "openai/gpt-realtime-whisper",
			sampleRate: 24000,
			apiKey: "sk-super-secret",
		};

		const redacted = redactCommand(command);

		expect(redacted).toEqual({
			...command,
			apiKey: "[REDACTED]",
		});
		expect(command.apiKey).toBe("sk-super-secret");
		expect(JSON.stringify(redacted)).not.toContain("sk-super-secret");
	});

	test("leaves commands without an API key untouched", () => {
		const command: InboundCommand = {
			type: "start",
			sessionId: "session-1",
			provider: "mistral",
			model: "voxtral-mini-transcribe-3-0",
			sampleRate: 16000,
		};

		expect(redactCommand(command)).toEqual(command);
	});

	test("returns non-start commands unchanged", () => {
		for (const command of [
			{ type: "audio", pcm16Base64: "AAAA" },
			{ type: "pause" },
			{ type: "resume" },
			{ type: "stop" },
		] as InboundCommand[]) {
			expect(redactCommand(command)).toEqual(command);
		}
	});
});

describe("capabilities parsing", () => {
	test("parses a capabilities request without optional fields", () => {
		const result = parseInboundLine(
			JSON.stringify({ type: "capabilities" }),
		);

		expect(result).toEqual({ ok: true, command: { type: "capabilities" } });
		if (!result.ok) return;
		expect("requestId" in result.command).toBe(false);
		expect("apiKey" in result.command).toBe(false);
	});

	test("parses requestId and apiKey when present", () => {
		const result = parseInboundLine(
			JSON.stringify({
				type: "capabilities",
				requestId: "cap-1",
				apiKey: "sk-secret",
			}),
		);

		expect(result).toEqual({
			ok: true,
			command: {
				type: "capabilities",
				requestId: "cap-1",
				apiKey: "sk-secret",
			},
		});
	});

	test("treats unexpected extra fields as non-fatal", () => {
		const result = parseInboundLine(
			JSON.stringify({ type: "capabilities", extra: true }),
		);

		expect(result).toEqual({ ok: true, command: { type: "capabilities" } });
	});

	test("rejects non-string requestId or apiKey with invalid_protocol", () => {
		const invalid: Array<Record<string, unknown>> = [
			{ requestId: 7 },
			{ requestId: null },
			{ requestId: "" },
			{ apiKey: 42 },
			{ apiKey: null },
			{ apiKey: ["sk-secret"] },
		];

		for (const overrides of invalid) {
			const result = parseInboundLine(
				JSON.stringify({ type: "capabilities", ...overrides }),
			);
			expect(result.ok).toBe(false);
			if (result.ok) continue;
			expect(result.code).toBe("invalid_protocol");
			expect(result.message).toContain("capabilities.");
		}
	});

	test("never echoes an api key in a parse error", () => {
		const result = parseInboundLine(
			JSON.stringify({ type: "capabilities", requestId: 7, apiKey: "sk-leak" }),
		);

		expect(result.ok).toBe(false);
		if (result.ok) return;
		expect(result.message).not.toContain("sk-leak");
	});
});

describe("capabilities serialization", () => {
	test("serializes the camelCase wire shape exactly", () => {
		const event: OutboundEvent = {
			type: "capabilities",
			requestId: "cap-1",
			source: "builtin",
			providers: [
				{
					id: "vercel-ai-gateway",
					label: "Vercel AI Gateway",
					available: true,
					keyEnvVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
					models: [
						{
							id: "openai/gpt-realtime-whisper",
							label: "gpt-realtime-whisper",
							mode: "streaming",
							description: null,
							pricing: {},
						},
						{
							id: "openai/gpt-4o-transcribe",
							label: "GPT-4o Transcribe",
							mode: "segmented",
							description: null,
							pricing: {},
						},
					],
				},
			],
		};

		const line = serializeOutboundEvent(event);

		expect(line).toBe(
			`${JSON.stringify({
				type: "capabilities",
				requestId: "cap-1",
				source: "builtin",
				providers: [
					{
						id: "vercel-ai-gateway",
						label: "Vercel AI Gateway",
						available: true,
						keyEnvVars: ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
						models: [
							{
								id: "openai/gpt-realtime-whisper",
								label: "gpt-realtime-whisper",
								mode: "streaming",
								description: null,
								pricing: {},
							},
							{
								id: "openai/gpt-4o-transcribe",
								label: "GPT-4o Transcribe",
								mode: "segmented",
								description: null,
								pricing: {},
							},
						],
					},
				],
			})}\n`,
		);
		expect(line.endsWith("\n")).toBe(true);
		expect(line.split("\n").filter(Boolean)).toHaveLength(1);
		expect(line).toContain('"keyEnvVars"');
		expect(line).toContain('"source":"builtin"');
	});

	test("serializes lowercase capability sources", () => {
		for (const source of ["builtin", "discovered"] as const) {
			const line = serializeOutboundEvent({
				type: "capabilities",
				source,
				providers: [],
			});

			expect(JSON.parse(line)).toEqual({
				type: "capabilities",
				source,
				providers: [],
			});
			expect(line).toContain(`"source":"${source}"`);
		}
	});

	test("omits requestId when the request had none", () => {
		const line = serializeOutboundEvent({
			type: "capabilities",
			source: "builtin",
			providers: [],
		});

		expect(line).toBe(
			'{"type":"capabilities","source":"builtin","providers":[]}\n',
		);
	});
});

describe("redactCommand capabilities", () => {
	test("masks the capabilities API key without mutating the input", () => {
		const command: CapabilitiesCommand = {
			type: "capabilities",
			requestId: "cap-1",
			apiKey: "sk-capability-secret",
		};

		const redacted = redactCommand(command);

		expect(redacted).toEqual({ ...command, apiKey: "[REDACTED]" });
		expect(command.apiKey).toBe("sk-capability-secret");
		expect(JSON.stringify(redacted)).not.toContain("sk-capability-secret");
	});

	test("leaves capabilities commands without a key untouched", () => {
		const command: CapabilitiesCommand = {
			type: "capabilities",
			requestId: "cap-1",
		};

		expect(redactCommand(command)).toEqual(command);
	});
});
