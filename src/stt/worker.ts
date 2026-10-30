/**
 * Bun subprocess STT worker.
 *
 * Reads NDJSON `InboundCommand` lines from `stdin` and writes `OutboundEvent`
 * lines to `stdout`. `stderr` is reserved for diagnostics.
 */
import { defaultSegmentedModel, MistralAdapter } from "./adapters/mistral";
import { createFileSession } from "./adapters/file";
import type { TranscriptionModel } from "ai";
import { transcribe as aiTranscribe } from "ai";
import type {
	SttAdapterFactory,
	SttSession,
	SttSessionConfig,
} from "./adapters/types";
import { sessionConfigFromStart } from "./adapters/types";
import { VercelAiSdkAdapter } from "./adapters/vercel-ai-sdk";
import {
	type DiscoveredModel,
	type DiscoveryResult,
	cachedGatewayModels,
	discoverGatewayModels,
} from "./discovery";
import {
	type FfmpegTools,
	FILE_SAMPLE_RATE,
	decodeFileToPcm16,
	findFfmpeg as defaultFindFfmpeg,
	probeDuration,
} from "./file-input";
import {
	type InboundCommand,
	type OutboundEvent,
	type TranscribeFileCommand,
	parseInboundLine,
	serializeOutboundEvent,
} from "./protocol";
import {
	getProvider,
	listCapabilities,
	resolveModelCapability,
	resolveProviderApiKey,
} from "./registry";

/**
 * Discovery seam for the capabilities handshake. The default source is the
 * anonymous gateway client; tests inject a deterministic one.
 */
export interface DiscoverySource {
	/** Freshly cached models, if any, without performing I/O. */
	cached(): readonly DiscoveredModel[] | undefined;
	/** Fetches the live catalogue; never receives an API key. */
	discover(): Promise<DiscoveryResult | undefined>;
}

export const defaultDiscoverySource: DiscoverySource = {
	cached: () => cachedGatewayModels(),
	discover: () => discoverGatewayModels(),
};

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

/**
 * File-session seam. A file session owns its own decoding, so the worker only
 * needs to resolve the ffmpeg toolchain and build the session.
 */
export interface FileDeps {
	/** Resolves `ffmpeg`/`ffprobe`; `undefined` when either is missing. */
	findFfmpeg(): FfmpegTools | undefined;
	/** Builds the session that transcribes `config.path`. */
	createSession(
		config: SttSessionConfig & { path: string },
		emit: (event: OutboundEvent) => void,
	): SttSession;
}

/** Per-platform install hint carried by the `ffmpeg_missing` error. */
export function ffmpegInstallHint(
	platform: NodeJS.Platform = process.platform,
): string {
	switch (platform) {
		case "darwin":
			return "install it with `brew install ffmpeg`";
		case "win32":
			return "install it with `winget install --id Gyan.FFmpeg -e`";
		default:
			return "install it with your package manager (apt/dnf/pacman install ffmpeg)";
	}
}

function defaultFileModel(config: SttSessionConfig): TranscriptionModel {
	return defaultSegmentedModel(config);
}

/** Real file-session wiring: ffmpeg decode + AI SDK batch transcription. */
export const defaultFileDeps: FileDeps = {
	findFfmpeg: () => defaultFindFfmpeg(),
	createSession: (config, emit) => {
		const tools = defaultFindFfmpeg();
		if (tools === undefined) {
			throw new Error("ffmpeg disappeared after it was resolved");
		}
		return createFileSession(config, emit, {
			decode: (path, { signal }) =>
				decodeFileToPcm16(path, { ffmpeg: tools.ffmpeg, signal }),
			probeDuration: (path, signal) =>
				probeDuration(path, tools.ffprobe, { signal }),
			transcribe: aiTranscribe,
			resolveModel: defaultFileModel,
		});
	},
};

export async function runWorkerLoop(
	lines: AsyncIterable<string>,
	emit: (event: OutboundEvent) => void,
	adapterFactory: SttAdapterFactory = defaultAdapterFactory,
	discovery: DiscoverySource = defaultDiscoverySource,
	files: FileDeps = defaultFileDeps,
): Promise<void> {
	let active: SttSession | undefined;
	/** A file session never receives live audio, so routing must know the kind. */
	let activeKind: "live" | "file" | undefined;
	let closed = false;

	const stopActive = async (): Promise<void> => {
		const session = active;
		active = undefined;
		activeKind = undefined;
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
		activeKind = undefined;
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
			activeKind = "live";
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

	/**
	 * Starts a file session. Validation mirrors `start` (provider, key), then
	 * the ffmpeg toolchain is resolved **before** any session exists so a
	 * missing binary produces `ffmpeg_missing` and no `started` event.
	 */
	const handleTranscribeFile = async (
		command: TranscribeFileCommand,
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

		const apiKey = resolveProviderApiKey(command.provider, command.apiKey);
		if (apiKey === undefined) {
			const provider = getProvider(command.provider);
			const envHint = provider?.envVars.join(" or ") ?? "the provider key";
			emit({
				type: "error",
				sessionId: command.sessionId,
				code: "missing_api_key",
				message: `Missing API key for ${command.provider}; set ${envHint} or pass apiKey in transcribeFile`,
			});
			return;
		}

		if (files.findFfmpeg() === undefined) {
			emit({
				type: "error",
				sessionId: command.sessionId,
				code: "ffmpeg_missing",
				message: `ffmpeg and ffprobe are required to transcribe files; ${ffmpegInstallHint()}`,
			});
			return;
		}

		const config: SttSessionConfig & { path: string } = {
			sessionId: command.sessionId,
			provider: command.provider,
			model: command.model,
			sampleRate: FILE_SAMPLE_RATE,
			apiKey,
			path: command.path,
		};
		try {
			active = files.createSession(config, emit);
			activeKind = "file";
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
			case "capabilities": {
				// Read-only handshake: never touches the active session, and
				// works before one exists. The response is built from the
				// registry only; the supplied key is never echoed, and the key
				// is never passed to discovery either.
				const capabilities = (
					source: "builtin" | "discovered",
					gatewayModels?: readonly DiscoveredModel[],
				): OutboundEvent => ({
					type: "capabilities",
					...(command.requestId === undefined
						? {}
						: { requestId: command.requestId }),
					source,
					providers: listCapabilities({
						apiKeyFor: () => command.apiKey,
						gatewayModels,
					}),
				});

				const cached = discovery.cached();
				if (cached !== undefined && cached.length > 0) {
					emit(capabilities("discovered", cached));
					return;
				}

				// Answer immediately from the curated catalogue so Settings is
				// never blocked by the network, then refresh if discovery
				// succeeds while the worker is still consuming input.
				emit(capabilities("builtin"));
				void discovery.discover().then(
					(result) => {
						if (closed || result === undefined || result.models.length === 0) {
							return;
						}
						emit(capabilities("discovered", result.models));
					},
					() => {
						// Discovery is best-effort: keep the builtin catalogue.
					},
				);
				return;
			}

			case "start":
				await handleStart(command);
				return;

			case "transcribeFile":
				await handleTranscribeFile(command);
				return;

			case "audio":
				// A file session owns its own decoding: live audio is ignored.
				if (activeKind === "live") {
					active?.pushAudio(command.pcm16Base64);
				}
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

	closed = true;
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
