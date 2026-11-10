import { afterEach, describe, expect, spyOn, test } from "bun:test";
import { describeError, logFailure, sanitizeDiagnostic } from "./diagnostics";

/** Captures everything a call writes to stderr. */
function captureStderr(run: () => void): string[] {
	const lines: string[] = [];
	const spy = spyOn(console, "error").mockImplementation((...args: unknown[]) => {
		lines.push(args.map(String).join(" "));
	});
	try {
		run();
	} finally {
		spy.mockRestore();
	}
	return lines;
}

const savedEnv: Record<string, string | undefined> = {};

afterEach(() => {
	for (const [name, value] of Object.entries(savedEnv)) {
		if (value === undefined) {
			delete process.env[name];
		} else {
			process.env[name] = value;
		}
	}
});

describe("sanitizeDiagnostic", () => {
	test("collapses whitespace and control characters", () => {
		expect(sanitizeDiagnostic("a\n\tb\u0000c   d")).toBe("a b c d");
	});

	test("redacts credentials", () => {
		expect(sanitizeDiagnostic("authorization: Bearer abc.def")).not.toContain("abc.def");
		expect(sanitizeDiagnostic("key=sk-secret-value")).not.toContain("sk-secret-value");
	});

	test("bounds the length", () => {
		expect(sanitizeDiagnostic("x".repeat(500)).length).toBeLessThanOrEqual(120);
	});
});

describe("describeError", () => {
	test("reads an Error message", () => {
		expect(describeError(new Error("model does not support streaming"), "fallback")).toBe(
			"model does not support streaming"
		);
	});

	test("reads a string rejection", () => {
		expect(describeError("provider down", "fallback")).toBe("provider down");
	});

	test("reads a nested message from a plain object", () => {
		// The AI SDK may reject with a record rather than an Error; reading only
		// `.message` on an Error collapsed every such failure to "…failed".
		expect(
			describeError({ name: "AI_APICallError", message: "unsupported option" }, "fallback")
		).toBe("unsupported option");
	});

	test("reads a wrapper that carries the real error one level down", () => {
		expect(describeError({ error: { message: "gateway rejected language" } }, "fallback")).toBe(
			"gateway rejected language"
		);
	});

	test("serializes an object it cannot otherwise read", () => {
		const described = describeError({ status: 400, code: "bad_request" }, "fallback");
		expect(described).toContain("400");
		expect(described).toContain("bad_request");
	});

	test("falls back only when nothing can be read", () => {
		expect(describeError(undefined, "Transcription stream failed")).toBe(
			"Transcription stream failed"
		);
		expect(describeError({}, "Transcription stream failed")).toBe(
			"Transcription stream failed"
		);
	});

	test("appends an Error cause", () => {
		const error = new Error("stream failed", {
			cause: new Error("socket closed")
		});
		const described = describeError(error, "fallback");
		expect(described).toContain("stream failed");
		expect(described).toContain("socket closed");
	});
});

describe("logFailure", () => {
	test("writes one redacted line naming the context and the subject", () => {
		const lines = captureStderr(() => {
			logFailure(
				"stream start",
				{
					provider: "vercel-ai-gateway",
					model: "google/gemini-3.5-transcribe-live",
					language: "fr"
				},
				new Error("Unsupported provider option: gateway.language"),
				"Transcription stream failed"
			);
		});

		expect(lines).toHaveLength(1);
		expect(lines[0]).toContain("context=stream start");
		expect(lines[0]).toContain("provider=vercel-ai-gateway");
		expect(lines[0]).toContain("model=google/gemini-3.5-transcribe-live");
		expect(lines[0]).toContain("language=fr");
		expect(lines[0]).toContain("gateway.language");
	});

	test("redacts a key the provider echoed back", () => {
		const lines = captureStderr(() => {
			logFailure("stream", {}, new Error("401 for key=sk-live-secret"), "failed");
		});

		expect(lines[0]).not.toContain("sk-live-secret");
		expect(lines[0]).toContain("[REDACTED]");
	});

	test("omits empty detail pairs", () => {
		const lines = captureStderr(() => {
			logFailure("batch", { provider: "mistral", language: undefined }, "boom", "failed");
		});

		expect(lines[0]).toContain("provider=mistral");
		expect(lines[0]).not.toContain("language=");
	});
});
