/**
 * Local media decoding for file transcription.
 *
 * The worker never receives audio bytes for a file session: it receives a path
 * and decodes the file itself with **ffmpeg**, so any container ffmpeg can
 * demux (audio or video) is supported. Output is canonicalised to 16 kHz mono
 * PCM16 on `stdout`, and every segment is later WAV-framed for the gateway —
 * the one container every transcription model accepts.
 *
 * `stderr` is diagnostic only and is truncated before it reaches an error
 * message. Nothing here writes to `stdout`.
 */

import type { SttErrorCode } from "./protocol";

/** Sample rate every file session is decoded to. */
export const FILE_SAMPLE_RATE = 16_000;

/** Channel count every file session is decoded to. */
export const FILE_CHANNELS = 1;

/** Cap on the stderr tail carried by a decode error. */
const STDERR_TAIL_LENGTH = 500;

/** Resolved ffmpeg toolchain. */
export interface FfmpegTools {
	readonly ffmpeg: string;
	readonly ffprobe: string;
}

function defaultWhich(binary: string): string | null {
	const found = Bun.which(binary);
	return found === null || found.length === 0 ? null : found;
}

function nonEmpty(value: string | undefined): string | undefined {
	const trimmed = value?.trim();
	return trimmed === undefined || trimmed.length === 0 ? undefined : trimmed;
}

/**
 * Resolves `ffmpeg`/`ffprobe` from the environment overrides or `PATH`.
 *
 * - `SPEECHTT_FFMPEG` / `SPEECHTT_FFPROBE` win when set and non-blank.
 * - Otherwise each binary is looked up on `PATH` independently.
 *
 * Returns `undefined` when either binary is missing: a file session cannot
 * report progress without `ffprobe`, and cannot decode at all without
 * `ffmpeg`.
 */
export function findFfmpeg(
	env: Record<string, string | undefined> = process.env,
	which: (binary: string) => string | null = defaultWhich,
): FfmpegTools | undefined {
	const ffmpeg = nonEmpty(env.SPEECHTT_FFMPEG) ?? which("ffmpeg");
	const ffprobe = nonEmpty(env.SPEECHTT_FFPROBE) ?? which("ffprobe");

	if (ffmpeg === null || ffprobe === null) {
		return undefined;
	}
	return { ffmpeg, ffprobe };
}

/** Result of a short, fully-buffered command execution. */
export interface ProcessResult {
	readonly exitCode: number;
	readonly stdout: string;
	readonly stderr: string;
}

/** Executes one command to completion. Injectable for tests. */
export type ExecFn = (
	argv: readonly string[],
	options?: { signal?: AbortSignal },
) => Promise<ProcessResult>;

/** A long-running decoder process. Injectable for tests. */
export interface SpawnedProcess {
	readonly stdout: AsyncIterable<Uint8Array>;
	readonly exited: Promise<number>;
	/** Resolves with the (already truncated) stderr tail once the process ends. */
	readonly stderr: Promise<string>;
	kill(signal?: number | NodeJS.Signals): void;
}

/** Starts a decoder subprocess. Injectable for tests. */
export type SpawnFn = (argv: readonly string[]) => SpawnedProcess;

function readStreamText(stream: ReadableStream<Uint8Array>): Promise<string> {
	return new Response(stream).text();
}

const defaultExec: ExecFn = async (argv, options) => {
	const process = Bun.spawn({
		cmd: [...argv],
		stdin: "ignore",
		stdout: "pipe",
		stderr: "pipe",
	});
	const onAbort = (): void => {
		process.kill("SIGTERM");
	};
	options?.signal?.addEventListener("abort", onAbort);

	try {
		const [stdout, stderr, exitCode] = await Promise.all([
			readStreamText(process.stdout as ReadableStream<Uint8Array>),
			readStreamText(process.stderr as ReadableStream<Uint8Array>),
			process.exited,
		]);
		return { exitCode, stdout, stderr };
	} finally {
		options?.signal?.removeEventListener("abort", onAbort);
	}
};

function toAsyncIterable(
	stream: ReadableStream<Uint8Array>,
): AsyncIterable<Uint8Array> {
	return {
		async *[Symbol.asyncIterator]() {
			const reader = stream.getReader();
			try {
				while (true) {
					const { done, value } = await reader.read();
					if (done) {
						return;
					}
					if (value !== undefined) {
						yield value;
					}
				}
			} finally {
				reader.releaseLock();
			}
		},
	};
}

function truncateTail(text: string): string {
	const trimmed = text.trim();
	return trimmed.length <= STDERR_TAIL_LENGTH
		? trimmed
		: trimmed.slice(-STDERR_TAIL_LENGTH);
}

const defaultSpawn: SpawnFn = (argv) => {
	const process = Bun.spawn({
		cmd: [...argv],
		stdin: "ignore",
		stdout: "pipe",
		stderr: "pipe",
	});

	return {
		stdout: toAsyncIterable(process.stdout as ReadableStream<Uint8Array>),
		exited: process.exited,
		stderr: readStreamText(
			process.stderr as ReadableStream<Uint8Array>,
		).then(truncateTail),
		kill: (signal) => {
			process.kill(signal);
		},
	};
};

/** Error codes a decode can raise. */
export type FileDecodeErrorCode = Extract<
	SttErrorCode,
	"file_not_found" | "unsupported_file" | "decode_failed"
>;

/** A decode failure with a wire-level code and a bounded diagnostic. */
export class FileDecodeError extends Error {
	constructor(
		readonly code: FileDecodeErrorCode,
		message: string,
	) {
		super(message);
		this.name = "FileDecodeError";
	}
}

const NOT_FOUND_PATTERNS: readonly RegExp[] = [
	/no such file or directory/i,
	/is a directory/i,
	/permission denied/i,
	/error opening input/i,
];

const UNSUPPORTED_PATTERNS: readonly RegExp[] = [
	/matches no streams/i,
	/does not contain any stream/i,
	/invalid data found/i,
	/moov atom not found/i,
	/could not find codec parameters/i,
	/unknown input format/i,
	/unsupported codec/i,
];

function classifyDecodeFailure(stderr: string): FileDecodeErrorCode {
	if (NOT_FOUND_PATTERNS.some((pattern) => pattern.test(stderr))) {
		return "file_not_found";
	}
	if (UNSUPPORTED_PATTERNS.some((pattern) => pattern.test(stderr))) {
		return "unsupported_file";
	}
	return "decode_failed";
}

/** argv that decodes the first audio stream to 16 kHz mono PCM16 on stdout. */
export function decodeArgs(path: string, ffmpeg: string): string[] {
	return [
		ffmpeg,
		"-nostdin",
		"-v",
		"error",
		"-i",
		path,
		"-map",
		"0:a:0",
		"-vn",
		"-ac",
		String(FILE_CHANNELS),
		"-ar",
		String(FILE_SAMPLE_RATE),
		"-f",
		"s16le",
		"-acodec",
		"pcm_s16le",
		"pipe:1",
	];
}

/** argv that reads only the container duration. */
export function probeArgs(path: string, ffprobe: string): string[] {
	return [
		ffprobe,
		"-v",
		"error",
		"-show_entries",
		"format=duration",
		"-of",
		"default=noprint_wrappers=1:nokey=1",
		path,
	];
}

/**
 * Reads the container duration in seconds.
 *
 * Best effort by design: every failure (missing tool, unparsable output,
 * non-zero exit, abort) resolves to `null`, because progress reporting must
 * never block a transcription.
 */
export async function probeDuration(
	path: string,
	ffprobe: string,
	options: { exec?: ExecFn; signal?: AbortSignal } = {},
): Promise<number | null> {
	const exec = options.exec ?? defaultExec;

	try {
		const result = await exec(probeArgs(path, ffprobe), {
			signal: options.signal,
		});
		if (result.exitCode !== 0) {
			return null;
		}

		const seconds = Number.parseFloat(result.stdout.trim());
		return Number.isFinite(seconds) && seconds > 0 ? seconds : null;
	} catch {
		return null;
	}
}

function concatBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
	const combined = new Uint8Array(left.length + right.length);
	combined.set(left, 0);
	combined.set(right, left.length);
	return combined;
}

/**
 * Streams 16 kHz mono PCM16 little-endian bytes decoded from `path`.
 *
 * Chunks are always even-length (PCM16 sample aligned); a trailing odd byte is
 * carried into the next chunk and a final stray byte is dropped.
 *
 * A non-zero ffmpeg exit after audio was already produced throws
 * [`FileDecodeError`] **after** the yielded chunks, so the caller can keep the
 * transcript it already built. An aborted signal kills the process and ends
 * the iterator silently.
 */
export async function* decodeFileToPcm16(
	path: string,
	options: { ffmpeg: string; spawn?: SpawnFn; signal?: AbortSignal },
): AsyncIterable<Uint8Array> {
	const spawn = options.spawn ?? defaultSpawn;
	const signal = options.signal;
	const process = spawn(decodeArgs(path, options.ffmpeg));

	const onAbort = (): void => {
		process.kill("SIGTERM");
	};
	if (signal?.aborted === true) {
		onAbort();
	} else {
		signal?.addEventListener("abort", onAbort);
	}

	let carry: Uint8Array | undefined;

	try {
		for await (const raw of process.stdout) {
			if (signal?.aborted === true) {
				break;
			}

			const chunk = carry === undefined ? raw : concatBytes(carry, raw);
			carry = undefined;

			if (chunk.length % 2 === 1) {
				carry = chunk.slice(chunk.length - 1);
			}

			const aligned =
				carry === undefined ? chunk : chunk.subarray(0, chunk.length - 1);
			if (aligned.length > 0) {
				yield aligned;
			}
		}
	} finally {
		signal?.removeEventListener("abort", onAbort);
	}

	if (signal?.aborted === true) {
		return;
	}

	const exitCode = await process.exited;
	if (exitCode === 0) {
		return;
	}

	// Classify against the full stderr (a marker may sit at the head), but carry
	// only a bounded tail into the message so a noisy decode cannot flood logs.
	const stderr = await process.stderr;
	const tail = truncateTail(stderr);
	throw new FileDecodeError(
		classifyDecodeFailure(stderr),
		`ffmpeg exited with code ${exitCode}${tail.length === 0 ? "" : `: ${tail}`}`,
	);
}
