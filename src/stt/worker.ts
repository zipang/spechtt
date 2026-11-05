/**
 * Bun subprocess STT worker.
 *
 * Reads NDJSON `InboundCommand` lines from `stdin` and writes `OutboundEvent`
 * lines to `stdout`. `stderr` is reserved for diagnostics, which the Rust host
 * surfaces in its trace.
 *
 * Every route lives in [`runWorkerLoop`]: transcription (`start`,
 * `transcribeFile`, `audio`, `pause`, `resume`, `stop`), the cleanup pass
 * (`cleanTranscript`), and the read-only capabilities handshake.
 */
import { defaultSegmentedModel, MistralAdapter } from "./adapters/mistral";
import { createFileSession } from "./adapters/file";
import type { TranscriptionModel } from "ai";
import { createGateway, generateText, transcribe as aiTranscribe } from "ai";
import type {
	SttAdapterFactory,
	SttSession,
	SttSessionConfig,
} from "./adapters/types";
import { sessionConfigFromStart } from "./adapters/types";
import { VercelAiSdkAdapter } from "./adapters/vercel-ai-sdk";
import type { CleanDeps } from "./clean";
import { cleanTranscript } from "./clean";
import {
	type DiscoveryResult,
	cachedGatewayCatalogue,
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
	type CleanTranscriptCommand,
	type InboundCommand,
	type OutboundEvent,
	type SttErrorCode,
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
	/** Freshly cached catalogue, if any, without performing I/O. */
	cached(): DiscoveryResult | undefined;
	/** Fetches the live catalogue; never receives an API key. */
	discover(): Promise<DiscoveryResult | undefined>;
}

export const defaultDiscoverySource: DiscoverySource = {
	cached: () => cachedGatewayCatalogue(),
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

/** Length of the head and tail probe used to spot a transcript in a message. */
const TRANSCRIPT_PROBE_LENGTH = 24;

/**
 * A cleanup failure message that never carries the transcript.
 *
 * [`runWorkerLoop`] calls this for the `cleanup_failed` event: provider errors
 * can quote the request they rejected, so a message containing the transcript
 * — or enough of its start or end to identify it — is replaced with a generic
 * one. Fragments shorter than eight characters are ignored, because they match
 * ordinary prose too easily.
 */
export function cleanupFailureMessage(
	error: unknown,
	transcript: string,
): string {
	const message = errorMessage(error);
	const probe = transcript.trim();
	if (probe.length === 0) {
		return message;
	}

	const fragments = [
		probe,
		probe.slice(0, TRANSCRIPT_PROBE_LENGTH),
		probe.slice(-TRANSCRIPT_PROBE_LENGTH),
	];
	const echoes = fragments.some(
		(fragment) => fragment.length >= 8 && message.includes(fragment),
	);
	return echoes ? "Cleanup failed" : message;
}

import { sanitizeDiagnostic } from "./diagnostics";
import {
	formatSessionHeader,
	partialsTraceEnabled,
	tracePartial,
} from "./trace";

export { sanitizeDiagnostic } from "./diagnostics";

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

/**
 * Real cleanup wiring: the AI SDK `generateText` call, charged to the gateway
 * key the request resolved.
 *
 * The model argument arrives as a gateway id string, so the provider instance
 * is built from that key instead of the environment default. `temperature`
 * comes from the caller so the repair pass stays close to deterministic.
 */
export function defaultCleanDeps(apiKey: string): CleanDeps {
	return {
		generate: async (args) => {
			const { text } = await generateText({
				model: createGateway({ apiKey })(args.model),
				prompt: args.prompt,
				temperature: args.temperature,
			});
			return { text };
		},
	};
}

export async function runWorkerLoop(
	lines: AsyncIterable<string>,
	emit: (event: OutboundEvent) => void,
	adapterFactory: SttAdapterFactory = defaultAdapterFactory,
	discovery: DiscoverySource = defaultDiscoverySource,
	files: FileDeps = defaultFileDeps,
	clean?: CleanDeps,
	/** Enables the transcript dump; defaults to `SPEECHTT_STT_PARTIALS=1`. */
	partialsTrace: boolean = partialsTraceEnabled(),
): Promise<void> {
	/** Monotonic counter for the dump, so a log replays in emission order. */
	let traceSeq = 0;
	/** Emits one event and, when the dump is on, one line describing it. */
	const tracedEmit = (event: OutboundEvent): void => {
		if (partialsTrace) {
			traceSeq += 1;
			tracePartial(event, traceSeq);
		}
		emit(event);
	};

	let active: SttSession | undefined;
	/** A file session never receives live audio, so routing must know the kind. */
	let activeKind: "live" | "file" | undefined;
	let closed = false;

	/** Emits one `error` event and exactly one redacted stderr diagnostic. */
	const emitError = (
		sessionId: string | undefined,
		code: SttErrorCode,
		message: string,
		requestId?: string,
	): void => {
		console.error(
			`[stt-worker] error code=${code} sessionId=${sessionId ?? "-"} requestId=${requestId ?? "-"} message="${sanitizeDiagnostic(message)}"`,
		);
		emit({
			type: "error",
			...(sessionId === undefined ? {} : { sessionId }),
			...(requestId === undefined ? {} : { requestId }),
			code,
			message,
		});
	};

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
			emitError(session.sessionId, "stream_failed", errorMessage(error));
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
			emitError(
				command.sessionId,
				"unsupported_provider",
				`Unsupported STT provider: ${command.provider}`,
			);
			return;
		}

		const apiKey = resolveProviderApiKey(
			command.provider,
			command.apiKey,
		);
		if (apiKey === undefined) {
			const provider = getProvider(command.provider);
			const envHint = provider?.envVars.join(" or ") ?? "the provider key";
			emitError(
				command.sessionId,
				"missing_api_key",
				`Missing API key for ${command.provider}; set ${envHint} or pass apiKey in start`,
			);
			return;
		}

		const config = sessionConfigFromStart(command, apiKey);
		if (partialsTrace) {
			console.error(
				formatSessionHeader({
					kind: "live",
					provider: config.provider,
					model: config.model,
					language: config.language,
					sampleRate: config.sampleRate,
				}),
			);
		}
		try {
			const adapter = await adapterFactory(config);
			active = await adapter.start(config, tracedEmit);
			activeKind = "live";
		} catch (error) {
			active = undefined;
			emitError(command.sessionId, "stream_failed", errorMessage(error));
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
			emitError(
				command.sessionId,
				"unsupported_provider",
				`Unsupported STT provider: ${command.provider}`,
			);
			return;
		}

		const apiKey = resolveProviderApiKey(command.provider, command.apiKey);
		if (apiKey === undefined) {
			const provider = getProvider(command.provider);
			const envHint = provider?.envVars.join(" or ") ?? "the provider key";
			emitError(
				command.sessionId,
				"missing_api_key",
				`Missing API key for ${command.provider}; set ${envHint} or pass apiKey in transcribeFile`,
			);
			return;
		}

		// A file take decodes locally and is transcribed in batches: a
		// streaming-only model has no endpoint for it, so refuse before any
		// session exists or ffmpeg is touched.
		if (resolveModelCapability(command.provider, command.model)?.mode === "streaming") {
			emitError(
				command.sessionId,
				"unsupported_file_model",
				`${command.model} cannot transcribe files; choose a non-streaming model`,
			);
			return;
		}

		if (files.findFfmpeg() === undefined) {
			emitError(
				command.sessionId,
				"ffmpeg_missing",
				`ffmpeg and ffprobe are required to transcribe files; ${ffmpegInstallHint()}`,
			);
			return;
		}

		const config: SttSessionConfig & { path: string } = {
			sessionId: command.sessionId,
			provider: command.provider,
			model: command.model,
			sampleRate: FILE_SAMPLE_RATE,
			apiKey,
			path: command.path,
			...(command.language === undefined
				? {}
				: { language: command.language }),
		};
		if (partialsTrace) {
			console.error(
				formatSessionHeader({
					kind: "file",
					provider: config.provider,
					model: config.model,
					language: config.language,
					sampleRate: config.sampleRate,
				}),
			);
		}
		try {
			active = files.createSession(config, tracedEmit);
			activeKind = "file";
		} catch (error) {
			active = undefined;
			emitError(command.sessionId, "stream_failed", errorMessage(error));
		}
	};

	/**
	 * Repairs the transcript of a finished take. It never touches the active
	 * session: the answer is correlated by `requestId`, so a cleanup can run
	 * while a new take is recording.
	 */
	const handleCleanTranscript = async (
		command: CleanTranscriptCommand,
	): Promise<void> => {
		const apiKey = resolveProviderApiKey(command.provider, command.apiKey);
		if (apiKey === undefined) {
			const provider = getProvider(command.provider);
			const envHint = provider?.envVars.join(" or ") ?? "the provider key";
			emitError(
				undefined,
				"missing_api_key",
				`Missing API key for ${command.provider}; set ${envHint} or pass apiKey in cleanTranscript`,
				command.requestId,
			);
			return;
		}

		const deps = clean ?? defaultCleanDeps(apiKey);
		try {
			const text = await cleanTranscript(
				{
					provider: command.provider,
					model: command.model,
					text: command.text,
					...(command.language === undefined
						? {}
						: { language: command.language }),
					apiKey,
				},
				deps,
			);
			emit({ type: "cleaned", requestId: command.requestId, text });
		} catch (error) {
			emitError(
				undefined,
				"cleanup_failed",
				cleanupFailureMessage(error, command.text),
				command.requestId,
			);
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
					catalogue?: DiscoveryResult,
				): OutboundEvent => {
					const payload = listCapabilities({
						apiKeyFor: () => command.apiKey,
						gatewayModels: catalogue?.models,
						languageModels: catalogue?.languageModels,
					});
					return {
						type: "capabilities",
						...(command.requestId === undefined
							? {}
							: { requestId: command.requestId }),
						source,
						providers: payload.providers,
						languages: payload.languages,
						languageModels: payload.languageModels,
						defaultLanguageModel: payload.defaultLanguageModel,
					};
				};

				const cached = discovery.cached();
				if (cached !== undefined && cached.models.length > 0) {
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
						emit(capabilities("discovered", result));
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

			case "cleanTranscript":
				await handleCleanTranscript(command);
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
			emitError(undefined, parsed.code, parsed.message);
			continue;
		}

		try {
			await handleCommand(parsed.command);
		} catch (error) {
			emitError(active?.sessionId, "stream_failed", errorMessage(error));
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
