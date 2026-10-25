import { describe, expect, test } from "bun:test";
import {
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
