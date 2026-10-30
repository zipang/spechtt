import { describe, expect, test } from "bun:test";
import {
	FileDecodeError,
	type ExecFn,
	type SpawnedProcess,
	decodeArgs,
	decodeFileToPcm16,
	findFfmpeg,
	probeDuration,
	probeArgs,
} from "./file-input";

/** Collects everything a decoder iterator yields. */
async function collect(
	iterator: AsyncIterable<Uint8Array>,
): Promise<Uint8Array[]> {
	const chunks: Uint8Array[] = [];
	for await (const chunk of iterator) {
		chunks.push(chunk);
	}
	return chunks;
}

function concat(chunks: readonly Uint8Array[]): Uint8Array {
	const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
	const output = new Uint8Array(total);
	let offset = 0;
	for (const chunk of chunks) {
		output.set(chunk, offset);
		offset += chunk.length;
	}
	return output;
}

async function* streamOf(...chunks: readonly Uint8Array[]) {
	for (const chunk of chunks) {
		yield chunk;
	}
}

interface FakeProcessOptions {
	readonly chunks?: readonly Uint8Array[];
	readonly exitCode?: number;
	readonly stderr?: string;
	readonly onKill?: (signal: string | number | undefined) => void;
}

function fakeProcess(options: FakeProcessOptions = {}): SpawnedProcess {
	return {
		stdout: streamOf(...(options.chunks ?? [])),
		exited: Promise.resolve(options.exitCode ?? 0),
		stderr: Promise.resolve(options.stderr ?? ""),
		kill: (signal) => options.onKill?.(signal),
	};
}

/** Runs a decoder and returns either its chunks or the thrown error. */
async function runDecoder(
	process: SpawnedProcess,
	path = "/tmp/a.mp4",
	signal?: AbortSignal,
): Promise<{ chunks: Uint8Array[]; error?: FileDecodeError }> {
	try {
		const chunks = await collect(
			decodeFileToPcm16(path, { ffmpeg: "ffmpeg", spawn: () => process, signal }),
		);
		return { chunks };
	} catch (error) {
		if (error instanceof FileDecodeError) {
			return { chunks: [], error };
		}
		throw error;
	}
}

describe("findFfmpeg", () => {
	const whichWith = (bins: readonly string[]) => (bin: string) =>
		bins.includes(bin) ? `/usr/bin/${bin}` : null;

	test("prefers the environment overrides", () => {
		const tools = findFfmpeg(
			{ SPEECHTT_FFMPEG: "/opt/ff/bin/ffmpeg", SPEECHTT_FFPROBE: "/opt/ff/bin/ffprobe" },
			whichWith(["ffmpeg", "ffprobe"]),
		);

		expect(tools).toEqual({
			ffmpeg: "/opt/ff/bin/ffmpeg",
			ffprobe: "/opt/ff/bin/ffprobe",
		});
	});

	test("falls back to PATH for each binary independently", () => {
		expect(
			findFfmpeg({ SPEECHTT_FFMPEG: "/opt/ffmpeg" }, whichWith(["ffprobe"])),
		).toEqual({ ffmpeg: "/opt/ffmpeg", ffprobe: "/usr/bin/ffprobe" });

		expect(
			findFfmpeg({ SPEECHTT_FFPROBE: "/opt/ffprobe" }, whichWith(["ffmpeg"])),
		).toEqual({ ffmpeg: "/usr/bin/ffmpeg", ffprobe: "/opt/ffprobe" });
	});

	test("returns undefined when either binary is absent", () => {
		expect(findFfmpeg({}, whichWith(["ffmpeg"]))).toBeUndefined();
		expect(findFfmpeg({}, whichWith(["ffprobe"]))).toBeUndefined();
		expect(findFfmpeg({}, whichWith([]))).toBeUndefined();
	});

	test("ignores blank overrides", () => {
		expect(
			findFfmpeg({ SPEECHTT_FFMPEG: "   " }, whichWith(["ffmpeg", "ffprobe"])),
		).toEqual({ ffmpeg: "/usr/bin/ffmpeg", ffprobe: "/usr/bin/ffprobe" });
	});

	test("does not consult PATH when both overrides are set", () => {
		let calls = 0;
		findFfmpeg(
			{ SPEECHTT_FFMPEG: "/a", SPEECHTT_FFPROBE: "/b" },
			() => {
				calls += 1;
				return null;
			},
		);

		expect(calls).toBe(0);
	});
});

describe("decodeArgs", () => {
	test("decodes the first audio stream to 16 kHz mono PCM16 on stdout", () => {
		expect(decodeArgs("/tmp/clip.mp4", "ffmpeg")).toEqual([
			"ffmpeg",
			"-nostdin",
			"-v",
			"error",
			"-i",
			"/tmp/clip.mp4",
			"-map",
			"0:a:0",
			"-vn",
			"-ac",
			"1",
			"-ar",
			"16000",
			"-f",
			"s16le",
			"-acodec",
			"pcm_s16le",
			"pipe:1",
		]);
	});

	test("keeps the configured binary path", () => {
		expect(decodeArgs("/tmp/a.wav", "/opt/ffmpeg")[0]).toBe("/opt/ffmpeg");
	});
});

describe("probeArgs", () => {
	test("reads only the format duration", () => {
		expect(probeArgs("/tmp/clip.mp4", "ffprobe")).toEqual([
			"ffprobe",
			"-v",
			"error",
			"-show_entries",
			"format=duration",
			"-of",
			"default=noprint_wrappers=1:nokey=1",
			"/tmp/clip.mp4",
		]);
	});
});

describe("probeDuration", () => {
	const execReturning = (result: Awaited<ReturnType<ExecFn>>): ExecFn =>
		async () => result;

	test("parses the duration in seconds", async () => {
		await expect(
			probeDuration("/tmp/a.mp4", "ffprobe", {
				exec: execReturning({ exitCode: 0, stdout: "340.250000\n", stderr: "" }),
			}),
		).resolves.toBe(340.25);
	});

	test("returns null for an unusable reading", async () => {
		for (const result of [
			{ exitCode: 0, stdout: "N/A\n", stderr: "" },
			{ exitCode: 0, stdout: "0\n", stderr: "" },
			{ exitCode: 0, stdout: "\n", stderr: "" },
			{ exitCode: 1, stdout: "12\n", stderr: "boom" },
		]) {
			await expect(
				probeDuration("/tmp/a.mp4", "ffprobe", { exec: execReturning(result) }),
			).resolves.toBeNull();
		}
	});

	test("never throws when the probe fails", async () => {
		await expect(
			probeDuration("/tmp/a.mp4", "ffprobe", {
				exec: async () => {
					throw new Error("spawn failed");
				},
			}),
		).resolves.toBeNull();
	});

	test("passes the abort signal through", async () => {
		const controller = new AbortController();
		let seen: AbortSignal | undefined;
		await probeDuration("/tmp/a.mp4", "ffprobe", {
			exec: async (_argv, options) => {
				seen = options?.signal;
				return { exitCode: 0, stdout: "1\n", stderr: "" };
			},
			signal: controller.signal,
		});

		expect(seen).toBe(controller.signal);
	});
});

describe("decodeFileToPcm16", () => {
	test("yields PCM16 chunks unchanged while the exit is clean", async () => {
		const result = await runDecoder(
			fakeProcess({ chunks: [new Uint8Array([1, 0, 2, 0]), new Uint8Array([3, 0])] }),
		);

		expect(result.error).toBeUndefined();
		expect(concat(result.chunks)).toEqual(new Uint8Array([1, 0, 2, 0, 3, 0]));
	});

	test("carries an odd trailing byte into the next chunk", async () => {
		const result = await runDecoder(
			fakeProcess({
				chunks: [
					new Uint8Array([1, 0, 2]),
					new Uint8Array([0, 3]),
					new Uint8Array([0, 4, 0]),
				],
			}),
		);

		expect(result.chunks.every((chunk) => chunk.length % 2 === 0)).toBe(true);
		expect(concat(result.chunks)).toEqual(new Uint8Array([1, 0, 2, 0, 3, 0, 4, 0]));
	});

	test("drops a single leftover byte at the end of the stream", async () => {
		const result = await runDecoder(
			fakeProcess({ chunks: [new Uint8Array([1, 0, 2])] }),
		);

		expect(concat(result.chunks)).toEqual(new Uint8Array([1, 0]));
	});

	test("maps a missing file to file_not_found", async () => {
		const result = await runDecoder(
			fakeProcess({
				exitCode: 1,
				stderr: "/tmp/a.mp4: No such file or directory",
			}),
		);

		expect(result.error?.code).toBe("file_not_found");
	});

	test("maps a directory to file_not_found", async () => {
		const result = await runDecoder(
			fakeProcess({ exitCode: 1, stderr: "/tmp: Is a directory" }),
		);

		expect(result.error?.code).toBe("file_not_found");
	});

	test("maps containers without an audio stream to unsupported_file", async () => {
		for (const stderr of [
			"Stream map '0:a:0' matches no streams.",
			"Invalid data found when processing input",
			"moov atom not found",
			"Could not find codec parameters for stream",
		]) {
			const result = await runDecoder(fakeProcess({ exitCode: 1, stderr }));

			expect(result.error?.code).toBe("unsupported_file");
		}
	});

	test("maps any other failure to decode_failed with a truncated stderr tail", async () => {
		const stderr = `${"x".repeat(600)}tail-marker`;
		const result = await runDecoder(fakeProcess({ exitCode: 1, stderr }));

		expect(result.error?.code).toBe("decode_failed");
		expect(result.error?.message).toContain("tail-marker");
		expect(result.error?.message).not.toContain("x".repeat(600));
	});

	test("keeps the chunks decoded before a mid-file failure", async () => {
		const process = fakeProcess({
			chunks: [new Uint8Array([7, 0])],
			exitCode: 1,
			stderr: "Error while decoding stream #0:0",
		});

		const chunks: Uint8Array[] = [];
		let failure: FileDecodeError | undefined;
		try {
			for await (const chunk of decodeFileToPcm16("/tmp/a.mp4", {
				ffmpeg: "ffmpeg",
				spawn: () => process,
			})) {
				chunks.push(chunk);
			}
		} catch (error) {
			failure = error as FileDecodeError;
		}

		expect(concat(chunks)).toEqual(new Uint8Array([7, 0]));
		expect(failure?.code).toBe("decode_failed");
	});

	test("kills the process and ends quietly when aborted", async () => {
		const controller = new AbortController();
		const kills: (string | number | undefined)[] = [];
		let pulled = 0;

		const process: SpawnedProcess = {
			stdout: {
				async *[Symbol.asyncIterator]() {
					yield new Uint8Array([1, 0]);
					pulled += 1;
					controller.abort();
					yield new Uint8Array([2, 0]);
				},
			},
			exited: Promise.resolve(0),
			stderr: Promise.resolve(""),
			kill: (signal) => kills.push(signal),
		};

		const chunks: Uint8Array[] = [];
		for await (const chunk of decodeFileToPcm16("/tmp/a.mp4", {
			ffmpeg: "ffmpeg",
			spawn: () => process,
			signal: controller.signal,
		})) {
			chunks.push(chunk);
		}

		expect(pulled).toBe(1);
		expect(kills.length).toBeGreaterThan(0);
		expect(concat(chunks)).toEqual(new Uint8Array([1, 0]));
	});

	test("passes the decode argv to the spawner", async () => {
		let seen: readonly string[] = [];
		await runDecoder(
			fakeProcess({ chunks: [] }),
			"/tmp/clip.mkv",
		);
		await collect(
			decodeFileToPcm16("/tmp/clip.mkv", {
				ffmpeg: "/usr/bin/ffmpeg",
				spawn: (argv) => {
					seen = argv;
					return fakeProcess({ chunks: [] });
				},
			}),
		);

		expect(seen).toEqual(decodeArgs("/tmp/clip.mkv", "/usr/bin/ffmpeg"));
	});

	test("starts the spawner exactly once", async () => {
		let spawns = 0;
		await collect(
			decodeFileToPcm16("/tmp/a.mp4", {
				ffmpeg: "ffmpeg",
				spawn: () => {
					spawns += 1;
					return fakeProcess({ chunks: [new Uint8Array([1, 0])] });
				},
			}),
		);

		expect(spawns).toBe(1);
	});
});
