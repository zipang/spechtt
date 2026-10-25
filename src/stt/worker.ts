/**
 * Bun subprocess STT worker.
 *
 * Reads NDJSON `InboundCommand` lines from `stdin` and writes `OutboundEvent`
 * lines to `stdout`. `stderr` is reserved for diagnostics.
 */
import { MistralAdapter } from "./adapters/mistral";
import type {
	SttAdapterFactory,
	SttSession,
	SttSessionConfig,
} from "./adapters/types";
import { sessionConfigFromStart } from "./adapters/types";
import { VercelAiSdkAdapter } from "./adapters/vercel-ai-sdk";
import {
	type InboundCommand,
	type OutboundEvent,
	parseInboundLine,
	serializeOutboundEvent,
} from "./protocol";
import {
	getProvider,
	resolveModelCapability,
	resolveProviderApiKey,
} from "./registry";

function errorMessage(error: unknown): string {
	if (error instanceof Error && error.message.length > 0) {
		return error.message;
	}
	if (typeof error === "string" && error.length > 0) {
		return error;
	}
	return "STT worker failed";
}

/** Picks the streaming or segmented adapter from the model capability. */
export function defaultAdapterFactory(
	config: SttSessionConfig,
): ReturnType<SttAdapterFactory> {
	const capability = resolveModelCapability(config.provider, config.model);
	if (capability?.mode === "segmented") {
		return new MistralAdapter();
	}
	return new VercelAiSdkAdapter();
}

export async function runWorkerLoop(
	lines: AsyncIterable<string>,
	emit: (event: OutboundEvent) => void,
	adapterFactory: SttAdapterFactory = defaultAdapterFactory,
): Promise<void> {
	let active: SttSession | undefined;

	const stopActive = async (): Promise<void> => {
		const session = active;
		active = undefined;
		if (session === undefined) {
			return;
		}

		try {
			await session.stop();
		} catch (error) {
			emit({
				type: "error",
				sessionId: session.sessionId,
				code: "stream_failed",
				message: errorMessage(error),
			});
		}
	};

	const abortActive = async (): Promise<void> => {
		const session = active;
		active = undefined;
		if (session === undefined) {
			return;
		}

		try {
			await session.abort(new Error("stdin closed"));
		} catch {
			// Shutdown must never throw.
		}
	};

	const handleStart = async (
		command: Extract<InboundCommand, { type: "start" }>,
	): Promise<void> => {
		await stopActive();

		if (getProvider(command.provider) === undefined) {
			emit({
				type: "error",
				sessionId: command.sessionId,
				code: "unsupported_provider",
				message: `Unsupported STT provider: ${command.provider}`,
			});
			return;
		}

		const apiKey = resolveProviderApiKey(
			command.provider,
			command.apiKey,
		);
		if (apiKey === undefined) {
			const provider = getProvider(command.provider);
			const envHint = provider?.envVars.join(" or ") ?? "the provider key";
			emit({
				type: "error",
				sessionId: command.sessionId,
				code: "missing_api_key",
				message: `Missing API key for ${command.provider}; set ${envHint} or pass apiKey in start`,
			});
			return;
		}

		const config = sessionConfigFromStart(command, apiKey);
		try {
			const adapter = await adapterFactory(config);
			active = await adapter.start(config, emit);
		} catch (error) {
			active = undefined;
			emit({
				type: "error",
				sessionId: command.sessionId,
				code: "stream_failed",
				message: errorMessage(error),
			});
		}
	};

	const handleCommand = async (command: InboundCommand): Promise<void> => {
		switch (command.type) {
			case "start":
				await handleStart(command);
				return;

			case "audio":
				active?.pushAudio(command.pcm16Base64);
				return;

			case "pause":
				active?.pause();
				return;

			case "resume":
				active?.resume();
				return;

			case "stop":
				await stopActive();
				return;
		}
	};

	emit({ type: "ready" });

	for await (const line of lines) {
		const parsed = parseInboundLine(line);
		if (!parsed.ok) {
			emit({
				type: "error",
				code: parsed.code,
				message: parsed.message,
			});
			continue;
		}

		try {
			await handleCommand(parsed.command);
		} catch (error) {
			emit({
				type: "error",
				sessionId: active?.sessionId,
				code: "stream_failed",
				message: errorMessage(error),
			});
		}
	}

	await abortActive();
}

/** Yields decoded lines from a byte stream, preserving partial final lines. */
export async function* decodeLines(
	stream: ReadableStream<Uint8Array>,
): AsyncGenerator<string> {
	const decoder = new TextDecoder();
	let buffer = "";

	for await (const chunk of stream) {
		buffer += decoder.decode(chunk, { stream: true });

		let newline = buffer.indexOf("\n");
		while (newline >= 0) {
			const line = buffer.slice(0, newline).replace(/\r$/, "");
			buffer = buffer.slice(newline + 1);
			yield line;
			newline = buffer.indexOf("\n");
		}
	}

	buffer += decoder.decode();
	if (buffer.length > 0) {
		yield buffer.replace(/\r$/, "");
	}
}

if (import.meta.main) {
	const emit = (event: OutboundEvent): void => {
		process.stdout.write(serializeOutboundEvent(event));
	};

	try {
		await runWorkerLoop(decodeLines(Bun.stdin.stream()), emit);
	} catch (error) {
		console.error("[stt-worker] fatal:", errorMessage(error));
		process.exitCode = 1;
	}
}
