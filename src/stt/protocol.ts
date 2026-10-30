/**
 * NDJSON wire protocol shared with `crates/speechtt-core/src/protocol.rs`.
 *
 * `stdin` carries `InboundCommand` lines (Rust -> Bun) and `stdout` carries
 * `OutboundEvent` lines (Bun -> Rust). Every message is one JSON object
 * terminated by `\n`. Nothing in this module may write to `stdout`.
 */

import type { ProviderCapabilityInfo } from "./registry";

export type SttErrorCode =
	| "invalid_protocol"
	| "missing_api_key"
	| "unsupported_provider"
	| "stream_failed"
	| "aborted"
	/** The requested path does not exist, is a directory, or is unreadable. */
	| "file_not_found"
	/** ffmpeg cannot demux the file: no audio stream or unsupported container. */
	| "unsupported_file"
	/** ffmpeg started but failed mid-decode. */
	| "decode_failed"
	/** `ffmpeg`/`ffprobe` are not installed or not on PATH. */
	| "ffmpeg_missing";

export interface StartCommand {
	type: "start";
	sessionId: string;
	provider: string;
	model: string;
	sampleRate: number;
	apiKey?: string;
}

export interface AudioCommand {
	type: "audio";
	pcm16Base64: string;
}

/**
 * Transcribe a file that already exists on the worker's host.
 *
 * Only the path crosses the bridge: the worker decodes the file locally with
 * ffmpeg (any container it supports) at a fixed 16 kHz mono, so no sample rate
 * is transmitted.
 */
export interface TranscribeFileCommand {
	type: "transcribeFile";
	sessionId: string;
	provider: string;
	model: string;
	/** Absolute path to an audio or video file on the worker's host. */
	path: string;
	apiKey?: string;
}

export interface PauseCommand {
	type: "pause";
}

export interface ResumeCommand {
	type: "resume";
}

export interface StopCommand {
	type: "stop";
}

/**
 * Read-only capability handshake. `requestId` is echoed back for correlation;
 * `apiKey` lets the UI test a key that is typed but not exported yet. The key
 * never appears in the response.
 */
export interface CapabilitiesCommand {
	type: "capabilities";
	requestId?: string;
	apiKey?: string;
}

export type InboundCommand =
	| StartCommand
	| TranscribeFileCommand
	| AudioCommand
	| PauseCommand
	| ResumeCommand
	| StopCommand
	| CapabilitiesCommand;

export interface ReadyEvent {
	type: "ready";
}

export interface StartedEvent {
	type: "started";
	sessionId: string;
}

export interface PartialEvent {
	type: "partial";
	sessionId: string;
	text: string;
}

export interface DeltaEvent {
	type: "delta";
	sessionId: string;
	delta: string;
}

export interface CommittedEvent {
	type: "committed";
	sessionId: string;
	text: string;
}

export interface StoppedEvent {
	type: "stopped";
	sessionId: string;
	fullText: string;
}

export interface ProgressEvent {
	type: "progress";
	sessionId: string;
	/** Seconds of decoded audio consumed so far (monotonic, >= 0). */
	processedSeconds: number;
	/** ffprobe duration, or null when it cannot be determined. */
	totalSeconds: number | null;
}

export interface ErrorEvent {
	type: "error";
	sessionId?: string;
	code: SttErrorCode;
	message: string;
}

/** Provider/model catalogue returned by the read-only handshake. */
export interface CapabilitiesEvent {
	type: "capabilities";
	requestId?: string;
	/** `builtin` is the curated fallback; `discovered` came from the gateway. */
	source: CapabilitySource;
	providers: readonly ProviderCapabilityInfo[];
}

export type CapabilitySource = "builtin" | "discovered";

export type OutboundEvent =
	| ReadyEvent
	| StartedEvent
	| PartialEvent
	| DeltaEvent
	| CommittedEvent
	| StoppedEvent
	| ProgressEvent
	| ErrorEvent
	| CapabilitiesEvent;

export type ParseInboundResult =
	| { ok: true; command: InboundCommand }
	| { ok: false; code: "invalid_protocol"; message: string };

const BASE64_PATTERN = /^[A-Za-z0-9+/]*={0,2}$/;

function invalid(message: string): ParseInboundResult {
	return { ok: false, code: "invalid_protocol", message };
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
	return typeof value === "string" && value.length > 0;
}

function isBase64(value: unknown): value is string {
	return (
		typeof value === "string" &&
		value.length % 4 === 0 &&
		BASE64_PATTERN.test(value)
	);
}

function parseStart(payload: Record<string, unknown>): ParseInboundResult {
	if (!isNonEmptyString(payload.sessionId)) {
		return invalid("start.sessionId must be a non-empty string");
	}
	if (!isNonEmptyString(payload.provider)) {
		return invalid("start.provider must be a non-empty string");
	}
	if (!isNonEmptyString(payload.model)) {
		return invalid("start.model must be a non-empty string");
	}
	if (
		typeof payload.sampleRate !== "number" ||
		!Number.isInteger(payload.sampleRate) ||
		payload.sampleRate <= 0
	) {
		return invalid("start.sampleRate must be a positive integer");
	}
	if (payload.apiKey !== undefined && typeof payload.apiKey !== "string") {
		return invalid("start.apiKey must be a string when present");
	}

	const command: StartCommand = {
		type: "start",
		sessionId: payload.sessionId,
		provider: payload.provider,
		model: payload.model,
		sampleRate: payload.sampleRate,
	};
	if (typeof payload.apiKey === "string") {
		command.apiKey = payload.apiKey;
	}
	return { ok: true, command };
}

function parseTranscribeFile(
	payload: Record<string, unknown>,
): ParseInboundResult {
	if (!isNonEmptyString(payload.sessionId)) {
		return invalid("transcribeFile.sessionId must be a non-empty string");
	}
	if (!isNonEmptyString(payload.provider)) {
		return invalid("transcribeFile.provider must be a non-empty string");
	}
	if (!isNonEmptyString(payload.model)) {
		return invalid("transcribeFile.model must be a non-empty string");
	}
	if (
		!isNonEmptyString(payload.path) ||
		payload.path.trim().length === 0 ||
		payload.path.includes("\u0000")
	) {
		return invalid(
			"transcribeFile.path must be a non-empty path without NUL bytes",
		);
	}
	if (payload.apiKey !== undefined && typeof payload.apiKey !== "string") {
		return invalid("transcribeFile.apiKey must be a string when present");
	}

	const command: TranscribeFileCommand = {
		type: "transcribeFile",
		sessionId: payload.sessionId,
		provider: payload.provider,
		model: payload.model,
		path: payload.path,
	};
	if (typeof payload.apiKey === "string") {
		command.apiKey = payload.apiKey;
	}
	return { ok: true, command };
}

function parseCapabilities(
	payload: Record<string, unknown>,
): ParseInboundResult {
	if (
		payload.requestId !== undefined &&
		!isNonEmptyString(payload.requestId)
	) {
		return invalid(
			"capabilities.requestId must be a non-empty string when present",
		);
	}
	if (payload.apiKey !== undefined && typeof payload.apiKey !== "string") {
		return invalid("capabilities.apiKey must be a string when present");
	}

	const command: CapabilitiesCommand = { type: "capabilities" };
	if (typeof payload.requestId === "string") {
		command.requestId = payload.requestId;
	}
	if (typeof payload.apiKey === "string") {
		command.apiKey = payload.apiKey;
	}
	return { ok: true, command };
}

/**
 * Parses one NDJSON line into a validated command. Malformed input never
 * throws: callers translate `invalid_protocol` into an `error` event and keep
 * reading the next line.
 */
export function parseInboundLine(line: string): ParseInboundResult {
	const trimmed = line.trim();
	if (trimmed.length === 0) {
		return invalid("Empty command line");
	}

	let payload: unknown;
	try {
		payload = JSON.parse(trimmed);
	} catch {
		// Never echo the raw line: it may contain an API key.
		return invalid("Invalid JSON payload");
	}

	if (!isRecord(payload)) {
		return invalid("Command must be a JSON object");
	}

	switch (payload.type) {
		case "start":
			return parseStart(payload);

		case "audio":
			if (!isBase64(payload.pcm16Base64)) {
				return invalid("audio.pcm16Base64 must be a base64 string");
			}
			return {
				ok: true,
				command: { type: "audio", pcm16Base64: payload.pcm16Base64 },
			};

		case "pause":
			return { ok: true, command: { type: "pause" } };

		case "resume":
			return { ok: true, command: { type: "resume" } };

		case "stop":
			return { ok: true, command: { type: "stop" } };

		case "capabilities":
			return parseCapabilities(payload);

		case "transcribeFile":
			return parseTranscribeFile(payload);

		default:
			return invalid(
				typeof payload.type === "string"
					? `Unknown command type: ${payload.type}`
					: "Missing command type",
			);
	}
}

/** Serializes an outbound event as one NDJSON line. */
export function serializeOutboundEvent(event: OutboundEvent): string {
	return `${JSON.stringify(event)}\n`;
}

/**
 * Returns a copy of `command` safe for diagnostics. An API key carried by a
 * `start` or `capabilities` command is replaced with `"[REDACTED]"`; all other
 * commands pass through unchanged.
 */
export function redactCommand(command: InboundCommand): InboundCommand {
	switch (command.type) {
		case "start":
		case "transcribeFile":
		case "capabilities":
			if (command.apiKey === undefined) {
				return command;
			}
			return { ...command, apiKey: "[REDACTED]" };
		default:
			return command;
	}
}
